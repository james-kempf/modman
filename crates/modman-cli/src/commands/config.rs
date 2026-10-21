// SPDX-License-Identifier: GPL-3.0-or-later
//! `config [key [value]]` (legacy `cmd_config` L3980-4014).

use std::path::Path;

use anyhow::Result;
use clap::Args;
use modman_core::config::{self, ConfigKey};

use crate::context::Ctx;

#[derive(Args, Debug)]
pub struct ConfigArgs {
    /// Setting name; omit to show all.
    pub key: Option<String>,
    /// New value; omit to print the current one.
    #[arg(allow_hyphen_values = true)]
    pub value: Option<String>,
}

pub fn run(ctx: &mut Ctx, args: ConfigArgs) -> Result<()> {
    let out = ctx.out;
    let Some(key) = args.key else {
        out.heading("Configuration");
        for k in ConfigKey::ALL {
            let mut v = config::get(&ctx.store, &ctx.paths, k)?;
            if k == ConfigKey::Cdn && v.is_empty() {
                v = "auto".into();
            }
            println!("  {:<23} {v}  ({})", k.name(), k.describe());
        }
        println!();
        println!("  {}", out.dim("Change with: modman config <key> <value>"));
        return Ok(());
    };
    let key: ConfigKey = key.parse()?;
    let Some(value) = args.value else {
        println!("{}", config::get(&ctx.store, &ctx.paths, key)?);
        return Ok(());
    };
    let old = config::get(&ctx.store, &ctx.paths, key)?;
    let new = config::set(&ctx.store, &ctx.paths, key, &value)?;
    if key == ConfigKey::DataDir && new != old && dir_has_entries(Path::new(&old)) {
        out.warn(&format!(
            "Existing data in {old} is not moved automatically. Purge deployments first, then move it yourself:"
        ));
        println!("    mv '{old}' '{new}'");
    }
    out.success(&format!("{} = {new}", key.name()));
    Ok(())
}

fn dir_has_entries(p: &Path) -> bool {
    std::fs::read_dir(p).is_ok_and(|mut d| d.next().is_some())
}
