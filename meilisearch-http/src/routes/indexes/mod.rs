use actix_web::web::Data;
use actix_web::{web, HttpRequest, HttpResponse};
use index_scheduler::{IndexScheduler, Query};
use log::debug;
use meilisearch_types::error::ResponseError;
use meilisearch_types::milli::{self, FieldDistribution, Index};
use meilisearch_types::tasks::{KindWithContent, Status};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use crate::analytics::Analytics;
use crate::extractors::authentication::{policies::*, GuardedData};
use crate::extractors::sequential_extractor::SeqHandler;

use super::{Pagination, SummarizedTaskView};

pub mod documents;
pub mod search;
pub mod settings;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("")
            .route(web::get().to(list_indexes))
            .route(web::post().to(SeqHandler(create_index))),
    )
    .service(
        web::scope("/{index_uid}")
            .service(
                web::resource("")
                    .route(web::get().to(SeqHandler(get_index)))
                    .route(web::patch().to(SeqHandler(update_index)))
                    .route(web::delete().to(SeqHandler(delete_index))),
            )
            .service(web::resource("/stats").route(web::get().to(SeqHandler(get_index_stats))))
            .service(web::scope("/documents").configure(documents::configure))
            .service(web::scope("/search").configure(search::configure))
            .service(web::scope("/settings").configure(settings::configure)),
    );
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexView {
    pub uid: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub primary_key: Option<String>,
}

impl IndexView {
    fn new(uid: String, index: &Index) -> Result<IndexView, milli::Error> {
        let rtxn = index.read_txn()?;
        Ok(IndexView {
            uid,
            created_at: index.created_at(&rtxn)?,
            updated_at: index.updated_at(&rtxn)?,
            primary_key: index.primary_key(&rtxn)?.map(String::from),
        })
    }
}

pub async fn list_indexes(
    index_scheduler: GuardedData<ActionPolicy<{ actions::INDEXES_GET }>, Data<IndexScheduler>>,
    paginate: web::Query<Pagination>,
) -> Result<HttpResponse, ResponseError> {
    let search_rules = &index_scheduler.filters().search_rules;
    let indexes: Vec<_> = index_scheduler.indexes()?;
    let indexes = indexes
        .into_iter()
        .filter(|(name, _)| search_rules.is_index_authorized(name))
        .map(|(name, index)| IndexView::new(name, &index))
        .collect::<Result<Vec<_>, _>>()?;

    let ret = paginate.auto_paginate_sized(indexes.into_iter());

    debug!("returns: {:?}", ret);
    Ok(HttpResponse::Ok().json(ret))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IndexCreateRequest {
    uid: String,
    primary_key: Option<String>,
}

pub async fn create_index(
    index_scheduler: GuardedData<ActionPolicy<{ actions::INDEXES_CREATE }>, Data<IndexScheduler>>,
    body: web::Json<IndexCreateRequest>,
    req: HttpRequest,
    analytics: web::Data<dyn Analytics>,
) -> Result<HttpResponse, ResponseError> {
    let IndexCreateRequest { primary_key, uid } = body.into_inner();

    analytics.publish(
        "Index Created".to_string(),
        json!({ "primary_key": primary_key }),
        Some(&req),
    );

    let task = KindWithContent::IndexCreation {
        index_uid: uid,
        primary_key,
    };
    let task: SummarizedTaskView =
        tokio::task::spawn_blocking(move || index_scheduler.register(task))
            .await??
            .into();

    Ok(HttpResponse::Accepted().json(task))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(dead_code)]
pub struct UpdateIndexRequest {
    uid: Option<String>,
    primary_key: Option<String>,
}

pub async fn get_index(
    index_scheduler: GuardedData<ActionPolicy<{ actions::INDEXES_GET }>, Data<IndexScheduler>>,
    index_uid: web::Path<String>,
) -> Result<HttpResponse, ResponseError> {
    let index = index_scheduler.index(&index_uid)?;
    let index_view = IndexView::new(index_uid.into_inner(), &index)?;

    debug!("returns: {:?}", index_view);

    Ok(HttpResponse::Ok().json(index_view))
}

pub async fn update_index(
    index_scheduler: GuardedData<ActionPolicy<{ actions::INDEXES_UPDATE }>, Data<IndexScheduler>>,
    path: web::Path<String>,
    body: web::Json<UpdateIndexRequest>,
    req: HttpRequest,
    analytics: web::Data<dyn Analytics>,
) -> Result<HttpResponse, ResponseError> {
    debug!("called with params: {:?}", body);
    let body = body.into_inner();
    analytics.publish(
        "Index Updated".to_string(),
        json!({ "primary_key": body.primary_key}),
        Some(&req),
    );

    let task = KindWithContent::IndexUpdate {
        index_uid: path.into_inner(),
        primary_key: body.primary_key,
    };

    let task: SummarizedTaskView =
        tokio::task::spawn_blocking(move || index_scheduler.register(task))
            .await??
            .into();

    debug!("returns: {:?}", task);
    Ok(HttpResponse::Accepted().json(task))
}

pub async fn delete_index(
    index_scheduler: GuardedData<ActionPolicy<{ actions::INDEXES_DELETE }>, Data<IndexScheduler>>,
    index_uid: web::Path<String>,
) -> Result<HttpResponse, ResponseError> {
    let task = KindWithContent::IndexDeletion {
        index_uid: index_uid.into_inner(),
    };
    let task: SummarizedTaskView =
        tokio::task::spawn_blocking(move || index_scheduler.register(task))
            .await??
            .into();

    Ok(HttpResponse::Accepted().json(task))
}

pub async fn get_index_stats(
    index_scheduler: GuardedData<ActionPolicy<{ actions::STATS_GET }>, Data<IndexScheduler>>,
    index_uid: web::Path<String>,
    req: HttpRequest,
    analytics: web::Data<dyn Analytics>,
) -> Result<HttpResponse, ResponseError> {
    analytics.publish(
        "Stats Seen".to_string(),
        json!({ "per_index_uid": true }),
        Some(&req),
    );

    let stats = IndexStats::new((*index_scheduler).clone(), index_uid.into_inner());

    debug!("returns: {:?}", stats);
    Ok(HttpResponse::Ok().json(stats))
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    pub number_of_documents: u64,
    pub is_indexing: bool,
    pub field_distribution: FieldDistribution,
}

impl IndexStats {
    pub fn new(
        index_scheduler: Data<IndexScheduler>,
        index_uid: String,
    ) -> Result<Self, ResponseError> {
        // we check if there is currently a task processing associated with this index.
        let processing_task = index_scheduler.get_tasks(
            Query::default()
                .with_status(Status::Processing)
                .with_index(index_uid.clone())
                .with_limit(1),
        )?;
        let is_processing = !processing_task.is_empty();

        let index = index_scheduler.index(&index_uid)?;
        let rtxn = index.read_txn()?;
        Ok(IndexStats {
            number_of_documents: index.number_of_documents(&rtxn)?,
            is_indexing: is_processing,
            field_distribution: index.field_distribution(&rtxn)?,
        })
    }
}
