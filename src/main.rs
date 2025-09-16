use anyhow::Result;
use args::Args;
use clap::Parser;

use crate::args::Command;

mod args;
mod diff;
mod fix;
mod models;

#[tokio::main]
async fn main() -> Result<()> {
    Args::parse().execute().await
}
