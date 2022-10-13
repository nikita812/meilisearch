use milli::update::IndexDocumentsMethod;
use serde::{Deserialize, Serialize, Serializer};
use std::{
    fmt::{Display, Write},
    str::FromStr,
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    error::{Code, ResponseError},
    keys::Key,
    settings::{Settings, Unchecked},
    InstanceUid,
};

pub type TaskId = u32;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub uid: TaskId,

    #[serde(with = "time::serde::rfc3339")]
    pub enqueued_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub finished_at: Option<OffsetDateTime>,

    pub error: Option<ResponseError>,
    pub details: Option<Details>,

    pub status: Status,
    pub kind: KindWithContent,
}

impl Task {
    pub fn index_uid(&self) -> Option<&str> {
        use KindWithContent::*;

        match &self.kind {
            DumpExport { .. }
            | Snapshot
            | CancelTask { .. }
            | DeleteTasks { .. }
            | IndexSwap { .. } => None,
            DocumentImport { index_uid, .. }
            | DocumentDeletion { index_uid, .. }
            | DocumentClear { index_uid }
            | Settings { index_uid, .. }
            | IndexCreation { index_uid, .. }
            | IndexUpdate { index_uid, .. }
            | IndexDeletion { index_uid } => Some(index_uid),
        }
    }

    /// Return the list of indexes updated by this tasks.
    pub fn indexes(&self) -> Option<Vec<&str>> {
        use KindWithContent::*;

        match &self.kind {
            DumpExport { .. } | Snapshot | CancelTask { .. } | DeleteTasks { .. } => None,
            DocumentImport { index_uid, .. }
            | DocumentDeletion { index_uid, .. }
            | DocumentClear { index_uid }
            | Settings { index_uid, .. }
            | IndexCreation { index_uid, .. }
            | IndexUpdate { index_uid, .. }
            | IndexDeletion { index_uid } => Some(vec![index_uid]),
            IndexSwap { lhs, rhs } => Some(vec![lhs, rhs]),
        }
    }

    /// Return the content-uuid if there is one
    pub fn content_uuid(&self) -> Option<&Uuid> {
        match self.kind {
            KindWithContent::DocumentImport {
                ref content_file, ..
            } => Some(content_file),
            KindWithContent::DocumentDeletion { .. }
            | KindWithContent::DocumentClear { .. }
            | KindWithContent::Settings { .. }
            | KindWithContent::IndexDeletion { .. }
            | KindWithContent::IndexCreation { .. }
            | KindWithContent::IndexUpdate { .. }
            | KindWithContent::IndexSwap { .. }
            | KindWithContent::CancelTask { .. }
            | KindWithContent::DeleteTasks { .. }
            | KindWithContent::DumpExport { .. }
            | KindWithContent::Snapshot => None,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KindWithContent {
    DocumentImport {
        index_uid: String,
        primary_key: Option<String>,
        method: IndexDocumentsMethod,
        content_file: Uuid,
        documents_count: u64,
        allow_index_creation: bool,
    },
    DocumentDeletion {
        index_uid: String,
        documents_ids: Vec<String>,
    },
    DocumentClear {
        index_uid: String,
    },
    Settings {
        index_uid: String,
        new_settings: Settings<Unchecked>,
        is_deletion: bool,
        allow_index_creation: bool,
    },
    IndexDeletion {
        index_uid: String,
    },
    IndexCreation {
        index_uid: String,
        primary_key: Option<String>,
    },
    IndexUpdate {
        index_uid: String,
        primary_key: Option<String>,
    },
    IndexSwap {
        lhs: String,
        rhs: String,
    },
    CancelTask {
        tasks: Vec<TaskId>,
    },
    DeleteTasks {
        query: String,
        tasks: Vec<TaskId>,
    },
    DumpExport {
        dump_uid: String,
        keys: Vec<Key>,
        instance_uid: Option<InstanceUid>,
    },
    Snapshot,
}

impl KindWithContent {
    pub fn as_kind(&self) -> Kind {
        match self {
            KindWithContent::DocumentImport { .. } => Kind::DocumentImport,
            KindWithContent::DocumentDeletion { .. } => Kind::DocumentDeletion,
            KindWithContent::DocumentClear { .. } => Kind::DocumentClear,
            KindWithContent::Settings { .. } => Kind::Settings,
            KindWithContent::IndexCreation { .. } => Kind::IndexCreation,
            KindWithContent::IndexDeletion { .. } => Kind::IndexDeletion,
            KindWithContent::IndexUpdate { .. } => Kind::IndexUpdate,
            KindWithContent::IndexSwap { .. } => Kind::IndexSwap,
            KindWithContent::CancelTask { .. } => Kind::CancelTask,
            KindWithContent::DeleteTasks { .. } => Kind::DeleteTasks,
            KindWithContent::DumpExport { .. } => Kind::DumpExport,
            KindWithContent::Snapshot => Kind::Snapshot,
        }
    }

    pub fn indexes(&self) -> Option<Vec<&str>> {
        use KindWithContent::*;

        match self {
            DumpExport { .. } | Snapshot | CancelTask { .. } | DeleteTasks { .. } => None,
            DocumentImport { index_uid, .. }
            | DocumentDeletion { index_uid, .. }
            | DocumentClear { index_uid }
            | Settings { index_uid, .. }
            | IndexCreation { index_uid, .. }
            | IndexUpdate { index_uid, .. }
            | IndexDeletion { index_uid } => Some(vec![index_uid]),
            IndexSwap { lhs, rhs } => Some(vec![lhs, rhs]),
        }
    }

    /// Returns the default `Details` that correspond to this `KindWithContent`,
    /// `None` if it cannot be generated.
    pub fn default_details(&self) -> Option<Details> {
        match self {
            KindWithContent::DocumentImport {
                documents_count, ..
            } => Some(Details::DocumentAddition {
                received_documents: *documents_count,
                indexed_documents: Some(0),
            }),
            KindWithContent::DocumentDeletion {
                index_uid: _,
                documents_ids,
            } => Some(Details::DocumentDeletion {
                received_document_ids: documents_ids.len(),
                deleted_documents: None,
            }),
            KindWithContent::DocumentClear { .. } => Some(Details::ClearAll {
                deleted_documents: None,
            }),
            KindWithContent::Settings { new_settings, .. } => Some(Details::Settings {
                settings: new_settings.clone(),
            }),
            KindWithContent::IndexDeletion { .. } => None,
            KindWithContent::IndexCreation { primary_key, .. }
            | KindWithContent::IndexUpdate { primary_key, .. } => Some(Details::IndexInfo {
                primary_key: primary_key.clone(),
            }),
            KindWithContent::IndexSwap { .. } => {
                todo!()
            }
            KindWithContent::CancelTask { .. } => {
                None // TODO: check correctness of this return value
            }
            KindWithContent::DeleteTasks { query, tasks } => Some(Details::DeleteTasks {
                matched_tasks: tasks.len(),
                deleted_tasks: None,
                original_query: query.clone(),
            }),
            KindWithContent::DumpExport { .. } => None,
            KindWithContent::Snapshot => None,
        }
    }
}

impl From<&KindWithContent> for Option<Details> {
    fn from(kind: &KindWithContent) -> Self {
        match kind {
            KindWithContent::DocumentImport {
                documents_count, ..
            } => Some(Details::DocumentAddition {
                received_documents: *documents_count,
                indexed_documents: None,
            }),
            KindWithContent::DocumentDeletion { .. } => None,
            KindWithContent::DocumentClear { .. } => None,
            KindWithContent::Settings { new_settings, .. } => Some(Details::Settings {
                settings: new_settings.clone(),
            }),
            KindWithContent::IndexDeletion { .. } => None,
            KindWithContent::IndexCreation { primary_key, .. } => Some(Details::IndexInfo {
                primary_key: primary_key.clone(),
            }),
            KindWithContent::IndexUpdate { primary_key, .. } => Some(Details::IndexInfo {
                primary_key: primary_key.clone(),
            }),
            KindWithContent::IndexSwap { .. } => None,
            KindWithContent::CancelTask { .. } => None,
            KindWithContent::DeleteTasks { .. } => todo!(),
            KindWithContent::DumpExport { dump_uid, .. } => Some(Details::Dump {
                dump_uid: dump_uid.clone(),
            }),
            KindWithContent::Snapshot => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Enqueued,
    Processing,
    Succeeded,
    Failed,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Enqueued => write!(f, "enqueued"),
            Status::Processing => write!(f, "processing"),
            Status::Succeeded => write!(f, "succeeded"),
            Status::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for Status {
    type Err = ResponseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "enqueued" => Ok(Status::Enqueued),
            "processing" => Ok(Status::Processing),
            "succeeded" => Ok(Status::Succeeded),
            "failed" => Ok(Status::Failed),
            s => Err(ResponseError::from_msg(
                format!("`{}` is not a status. Available types are", s),
                Code::BadRequest,
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    DocumentImport,
    DocumentDeletion,
    DocumentClear,
    Settings,
    IndexCreation,
    IndexDeletion,
    IndexUpdate,
    IndexSwap,
    CancelTask,
    DeleteTasks,
    DumpExport,
    Snapshot,
}

impl FromStr for Kind {
    type Err = ResponseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "document_addition" => Ok(Kind::DocumentImport),
            "document_update" => Ok(Kind::DocumentImport),
            "document_deletion" => Ok(Kind::DocumentDeletion),
            "document_clear" => Ok(Kind::DocumentClear),
            "settings" => Ok(Kind::Settings),
            "index_creation" => Ok(Kind::IndexCreation),
            "index_deletion" => Ok(Kind::IndexDeletion),
            "index_update" => Ok(Kind::IndexUpdate),
            "index_swap" => Ok(Kind::IndexSwap),
            "cancel_task" => Ok(Kind::CancelTask),
            "delete_tasks" => Ok(Kind::DeleteTasks),
            "dump_export" => Ok(Kind::DumpExport),
            "snapshot" => Ok(Kind::Snapshot),
            s => Err(ResponseError::from_msg(
                format!("`{}` is not a type. Available status are ", s),
                Code::BadRequest,
            )),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum Details {
    DocumentAddition {
        received_documents: u64,
        indexed_documents: Option<u64>,
    },
    Settings {
        settings: Settings<Unchecked>,
    },
    IndexInfo {
        primary_key: Option<String>,
    },
    DocumentDeletion {
        received_document_ids: usize,
        // TODO why is this optional?
        deleted_documents: Option<u64>,
    },
    ClearAll {
        deleted_documents: Option<u64>,
    },
    DeleteTasks {
        matched_tasks: usize,
        deleted_tasks: Option<usize>,
        original_query: String,
    },
    Dump {
        dump_uid: String,
    },
}

/// Serialize a `time::Duration` as a best effort ISO 8601 while waiting for
/// https://github.com/time-rs/time/issues/378.
/// This code is a port of the old code of time that was removed in 0.2.
pub fn serialize_duration<S: Serializer>(
    duration: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match duration {
        Some(duration) => {
            // technically speaking, negative duration is not valid ISO 8601
            if duration.is_negative() {
                return serializer.serialize_none();
            }

            const SECS_PER_DAY: i64 = Duration::DAY.whole_seconds();
            let secs = duration.whole_seconds();
            let days = secs / SECS_PER_DAY;
            let secs = secs - days * SECS_PER_DAY;
            let hasdate = days != 0;
            let nanos = duration.subsec_nanoseconds();
            let hastime = (secs != 0 || nanos != 0) || !hasdate;

            // all the following unwrap can't fail
            let mut res = String::new();
            write!(&mut res, "P").unwrap();

            if hasdate {
                write!(&mut res, "{}D", days).unwrap();
            }

            const NANOS_PER_MILLI: i32 = Duration::MILLISECOND.subsec_nanoseconds();
            const NANOS_PER_MICRO: i32 = Duration::MICROSECOND.subsec_nanoseconds();

            if hastime {
                if nanos == 0 {
                    write!(&mut res, "T{}S", secs).unwrap();
                } else if nanos % NANOS_PER_MILLI == 0 {
                    write!(&mut res, "T{}.{:03}S", secs, nanos / NANOS_PER_MILLI).unwrap();
                } else if nanos % NANOS_PER_MICRO == 0 {
                    write!(&mut res, "T{}.{:06}S", secs, nanos / NANOS_PER_MICRO).unwrap();
                } else {
                    write!(&mut res, "T{}.{:09}S", secs, nanos).unwrap();
                }
            }

            serializer.serialize_str(&res)
        }
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use crate::heed::{types::SerdeJson, BytesDecode, BytesEncode};

    use super::Details;

    #[test]
    fn bad_deser() {
        let details = Details::DeleteTasks {
            matched_tasks: 1,
            deleted_tasks: None,
            original_query: "hello".to_owned(),
        };
        let serialised = SerdeJson::<Details>::bytes_encode(&details).unwrap();
        let deserialised = SerdeJson::<Details>::bytes_decode(&serialised).unwrap();
        meili_snap::snapshot!(format!("{:?}", details), @r###"DeleteTasks { matched_tasks: 1, deleted_tasks: None, original_query: "hello" }"###);
        meili_snap::snapshot!(format!("{:?}", deserialised), @r###"DeleteTasks { matched_tasks: 1, deleted_tasks: None, original_query: "hello" }"###);
    }
}
