use std::collections::BTreeMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use clap::Parser;
use openapiv3::OpenAPI;
use tokio::fs;

use crate::{
    args::Command,
    models::{OperationIdDiff, OperationIdDiffEntry},
};

#[derive(Debug, Parser)]
pub struct Diff {
    /// Before file
    #[arg(short, long)]
    before: Utf8PathBuf,

    /// After file
    #[arg(short, long)]
    after: Utf8PathBuf,

    /// Output file
    #[arg(short, long)]
    output: Utf8PathBuf,
}

#[async_trait]
impl Command for Diff {
    async fn execute(&self) -> Result<()> {
        let before = fs::read_to_string(&self.before)
            .await
            .context("read before file")?;
        let after = fs::read_to_string(&self.after)
            .await
            .context("read after file")?;

        let before_json = serde_json::from_str::<OpenAPI>(&before).context("parse before file")?;
        let after_json = serde_json::from_str::<OpenAPI>(&after).context("parse after file")?;

        let mut before_operation_ids = operation_ids(&before_json);
        let after_operation_ids = operation_ids(&after_json);

        let mut operation_id_diff = OperationIdDiff::default();

        for (key, (_tag_after, operation_id_after)) in after_operation_ids {
            if let Some((tag_before, operation_id_before)) = before_operation_ids.remove(&key) {
                if operation_id_before != operation_id_after {
                    operation_id_diff.entries.push(OperationIdDiffEntry {
                        tag: tag_before,
                        operation_id_before,
                        operation_id_after,
                    });
                }
            }
        }

        println!(
            "Number of operation id diffs: {:?}",
            operation_id_diff.entries.len()
        );

        let diff = serde_json::to_string_pretty(&operation_id_diff)
            .context("serialize operation id diff")?;
        fs::write(&self.output, diff)
            .await
            .context("write operation id diff")?;

        Ok(())
    }
}

fn operation_ids(openapi: &OpenAPI) -> BTreeMap<(String, String), (String, String)> {
    let mut operation_ids = BTreeMap::new();

    for (path, path_item) in openapi.paths.iter() {
        if let Some(path_item) = path_item.as_item() {
            for (verb, operation) in path_item.iter() {
                if let Some(operation_id) = operation.operation_id.as_ref() {
                    let operation_id = operation
                        .extensions
                        .get("x-xgen-operation-id-override")
                        .and_then(|v| v.as_str())
                        .unwrap_or(operation_id)
                        .to_string();

                    let Some(tag) = operation.tags.first() else {
                        continue;
                    };

                    operation_ids.insert(
                        (path.clone(), verb.to_string()),
                        (tag.clone(), operation_id.clone()),
                    );
                }
            }
        }
    }

    operation_ids
}
