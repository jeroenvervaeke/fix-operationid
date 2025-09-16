use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap, VecDeque},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use camino::Utf8PathBuf;
use clap::Parser;
use futures::stream::{self, StreamExt};
use tokio::fs;
use tree_sitter::{Query, QueryCursor, QueryMatch, StreamingIterator, Tree};

use crate::{args::Command, models::OperationIdDiff};

#[derive(Debug, Parser)]
pub struct Fix {
    /// Operation id diff file
    #[arg(short, long)]
    operation_id_diff: Utf8PathBuf,

    /// Test file
    #[arg(short, long)]
    cli_directory: Utf8PathBuf,

    /// Go SDK version
    #[arg(short, long)]
    go_sdk_version: String,
}

#[async_trait]
impl Command for Fix {
    async fn execute(&self) -> Result<()> {
        let operation_id_diff = fs::read_to_string(&self.operation_id_diff)
            .await
            .context("read operation id diff file")?;

        let operation_id_diff = serde_json::from_str::<OperationIdDiff>(&operation_id_diff)
            .context("parse operation id diff file")?;

        let operation_id_lookup = operation_id_diff
            .entries
            .iter()
            .map(|entry| {
                (
                    fix_naming(&entry.operation_id_before),
                    (
                        format!("{}Api", fix_naming(&entry.tag)),
                        fix_naming(&entry.operation_id_after),
                    ),
                )
            })
            .collect::<HashMap<String, (String, String)>>();

        let files = get_files_recursive(&self.cli_directory)
            .await
            .context("read store directory")?;

        stream::iter(files)
            .map(|file| async { fix_file(&operation_id_lookup, file, &self.go_sdk_version).await })
            .buffer_unordered(16) // Process up to 16 items concurrently
            .collect::<Vec<_>>()
            .await;

        Ok(())
    }
}

async fn get_files_recursive(directory: &Utf8PathBuf) -> Result<Vec<Utf8PathBuf>> {
    let mut files = Vec::new();
    let mut directories_to_process = VecDeque::new();
    directories_to_process.push_back(directory.clone());

    while let Some(directory) = directories_to_process.pop_front() {
        let mut read_dir = fs::read_dir(directory).await.context("read dir")?;

        while let Some(file) = read_dir.next_entry().await.context("read dir entry")? {
            let file_path = Utf8PathBuf::from_path_buf(file.path()).unwrap();

            if file_path.is_dir() {
                directories_to_process.push_back(file_path);
                continue;
            }

            if file_path.extension().unwrap_or_default() == "go" {
                files.push(file_path);
            }
        }
    }

    Ok(files)
}

fn fix_naming(value: &str) -> String {
    let mut result = String::new();
    let mut new_word = true;

    for c in value.chars() {
        if c == ' ' || c == '.' || c == '-' {
            new_word = true;
            continue;
        }

        if new_word {
            result.push(c.to_ascii_uppercase());
        } else {
            result.push(c);
        }

        new_word = false;
    }

    result
}

async fn fix_file(
    operation_id_lookup: &HashMap<String, (String, String)>,
    test_file: Utf8PathBuf,
    go_sdk_version: &str,
) -> Result<()> {
    let file_content = fs::read_to_string(&test_file)
        .await
        .context("read test file")?;

    let file_content = file_content.as_bytes();
    let mut edits = get_edits(file_content, operation_id_lookup, go_sdk_version)?;

    let mut file_content = file_content.to_vec();

    while let Some(edit) = edits.pop() {
        for i in (edit.start..edit.end).rev() {
            file_content.remove(i);
        }

        for (i, c) in edit.replacement.chars().enumerate() {
            file_content.insert(edit.start + i, c as u8);
        }
    }

    fs::write(test_file, &file_content)
        .await
        .context("write file")?;

    Ok(())
}

const WITH_PARAMS: &str = "WithParams";
const API_PARAMS: &str = "ApiParams";

fn get_edits(
    file_content: &[u8],
    operation_id_lookup: &HashMap<String, (String, String)>,
    go_sdk_version: &str,
) -> Result<BinaryHeap<Edit>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .context("set language")?;

    let tree = parser.parse(file_content, None).context("parse file")?;

    let mut edits = BinaryHeap::new();

    // Fix all the method calls
    let method_calls_edits = fix_method_calls(&tree, file_content, operation_id_lookup)?;
    edits.extend(method_calls_edits);

    // Fix all the api params
    let api_params_edits =
        fix_api_params(&tree, file_content, operation_id_lookup, go_sdk_version)?;
    edits.extend(api_params_edits);

    Ok(edits)
}

fn fix_method_calls(
    tree: &Tree,
    file_content: &[u8],
    operation_id_lookup: &HashMap<String, (String, String)>,
) -> Result<BinaryHeap<Edit>> {
    let mut edits = BinaryHeap::new();

    let query =
        Query::new(&tree.language(), include_str!("query_methods.csm")).context("create query")?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), file_content);

    let capture_index_store_name = get_capture_index(&query, "store_name")?;
    let capture_index_store_property = get_capture_index(&query, "store_property")?;
    let capture_index_tag = get_capture_index(&query, "tag")?;
    let capture_index_operation_id = get_capture_index(&query, "operation_id")?;

    while let Some(m) = matches.next() {
        let store_name = get_capture_value(file_content, &m, capture_index_store_name)?
            .context("get store name")?;
        let store_property = get_capture_value(file_content, &m, capture_index_store_property)?
            .context("get store property")?;

        if store_name == "s" && store_property == "clientv2" {
            let tag = get_capture_value(file_content, &m, capture_index_tag)?.context("get tag")?;
            let operation_id = get_capture_value(file_content, &m, capture_index_operation_id)?
                .context("get operation id")?;

            let ends_with_params = operation_id.ends_with(WITH_PARAMS);
            let operation_id = operation_id.trim_end_matches(WITH_PARAMS);

            if let Some((operation_tag, replacement)) = operation_id_lookup.get(operation_id) {
                if tag != *operation_tag {
                    println!(
                        "Skipping {} because it has tag {} but expected {}",
                        operation_id, tag, operation_tag
                    );
                    continue;
                }

                let (start, mut end) = get_capture_range(&m, capture_index_operation_id)
                    .context("get capture range")?;

                if ends_with_params {
                    end -= WITH_PARAMS.len()
                }

                edits.push(Edit {
                    start,
                    end,
                    replacement: replacement.clone(),
                });
            }
        }
    }

    Ok(edits)
}

fn fix_api_params(
    tree: &Tree,
    file_content: &[u8],
    operation_id_lookup: &HashMap<String, (String, String)>,
    go_sdk_version: &str,
) -> Result<BinaryHeap<Edit>> {
    // Get the go sdk import name
    let Some(go_sdk_import_name) = get_go_sdk_import_name(tree, file_content, go_sdk_version)
        .context("get go sdk import name")?
    else {
        return Ok(BinaryHeap::new());
    };

    let mut edits = BinaryHeap::new();

    let query =
        Query::new(&tree.language(), include_str!("query_params.csm")).context("create query")?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), file_content);

    let capture_index_package = get_capture_index(&query, "package")?;
    let capture_index_type = get_capture_index(&query, "type")?;

    while let Some(m) = matches.next() {
        let package =
            get_capture_value(file_content, &m, capture_index_package)?.context("get package")?;
        let type_identifier =
            get_capture_value(file_content, &m, capture_index_type)?.context("get type")?;

        if package == go_sdk_import_name && type_identifier.ends_with(API_PARAMS) {
            let clean_type_identifier = type_identifier.trim_end_matches(API_PARAMS);
            if let Some((_operation_tag, replacement)) =
                operation_id_lookup.get(clean_type_identifier)
            {
                let (start, mut end) =
                    get_capture_range(&m, capture_index_type).context("get capture range")?;

                end -= API_PARAMS.len();

                edits.push(Edit {
                    start,
                    end,
                    replacement: replacement.to_string(),
                });
            }
        }
    }

    Ok(edits)
}

fn get_go_sdk_import_name(
    tree: &Tree,
    file_content: &[u8],
    go_sdk_version: &str,
) -> Result<Option<String>> {
    let query =
        Query::new(&tree.language(), include_str!("query_imports.csm")).context("create query")?;
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), file_content);

    let capture_index_path = get_capture_index(&query, "path")?;
    let capture_index_name = get_capture_index(&query, "name")?;

    while let Some(m) = matches.next() {
        let path = get_capture_value(file_content, &m, capture_index_path)?.context("get path")?;
        let path = path.trim_matches('"');

        if path == go_sdk_version {
            let name = if let Some(name) = get_capture_value(file_content, &m, capture_index_name)?
            {
                name.trim_matches('"').to_string()
            } else {
                "admin".to_string()
            };

            return Ok(Some(name));
        }
    }

    Ok(None)
}

fn get_capture_index(query: &Query, name: &'static str) -> Result<u32> {
    query
        .capture_index_for_name(name)
        .context(format!("get capture index for {name}"))
}

fn get_capture_value(
    file_content: &[u8],
    query_match: &QueryMatch,
    capture_index: u32,
) -> Result<Option<String>> {
    query_match
        .captures
        .iter()
        .find(|capture| capture.index == capture_index)
        .map(|capture| {
            capture
                .node
                .utf8_text(file_content)
                .context("get capture value")
                .map(|value| value.to_string())
        })
        .transpose()
}

fn get_capture_range(query_match: &QueryMatch, capture_index: u32) -> Option<(usize, usize)> {
    query_match
        .captures
        .iter()
        .find(|capture| capture.index == capture_index)
        .map(|capture| (capture.node.start_byte(), capture.node.end_byte()))
}

#[derive(Debug, PartialEq, Eq)]
struct Edit {
    start: usize,
    end: usize,
    replacement: String,
}

impl Ord for Edit {
    fn cmp(&self, other: &Self) -> Ordering {
        self.start.cmp(&other.start)
    }
}

impl PartialOrd for Edit {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
