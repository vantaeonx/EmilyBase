//! Count-only native archive reports; no names, owners or payloads on stdout.
use emilybase_files::FileArchiveReport;

pub fn report(value: &FileArchiveReport) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = value.metadata();
    super::write_operator_metadata(&serde_json::json!({
        "format":1,
        "project":value.project().to_string(),
        "metadata":{
            "database_id":metadata.database_id.iter()
                .map(|byte|format!("{byte:02x}")).collect::<String>(),
            "last_transaction":metadata.last_transaction.to_string(),
            "wal_version":metadata.wal_version,
            "wal_bytes":metadata.wal_bytes,
            "tables":metadata.tables,"rows":metadata.rows,"pages":metadata.pages
        },
        "quota":{
            "max_objects":value.quota().objects(),
            "max_bytes":value.quota().payload_bytes()
        },
        "references":value.references(),
        "objects":{
            "count":value.objects().objects,
            "bytes":value.objects().payload_bytes,
            "digest":value.objects().digest.iter()
                .map(|byte|format!("{byte:02x}")).collect::<String>()
        }
    }))
}
