use crate::{diff, fix};
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use enum_dispatch::enum_dispatch;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
#[enum_dispatch(Command)]
pub enum Args {
    Diff(diff::Diff),
    Fix(fix::Fix),
}

#[async_trait]
#[enum_dispatch]
pub trait Command {
    async fn execute(&self) -> Result<()>;
}
