use anyhow::Result;
use std::collections::HashSet;

use super::{bundle, kubo, kubo_exec, Engine};

impl Engine {
    // ── Abot CRUD ────────────────────────────────────────────

    pub async fn list_abots(&self) -> Vec<bundle::AbotDetail> {
        let session_keys = self.build_session_keys().await;
        let abots = bundle::read_known_abots(&self.data_dir);
        abots
            .iter()
            .filter_map(|a| {
                let mut detail = bundle::get_abot_detail(&self.data_dir, &a.name).ok()?;
                detail.added_at = Some(a.added_at.clone());
                inject_has_session(&mut detail, &session_keys);
                Some(detail)
            })
            .collect()
    }

    pub async fn get_abot_info(&self, abot_name: &str) -> Result<bundle::AbotDetail> {
        kubo::validate_name(abot_name)?;
        let mut detail = bundle::get_abot_detail(&self.data_dir, abot_name)?;
        let session_keys = self.build_session_keys().await;
        inject_has_session(&mut detail, &session_keys);
        Ok(detail)
    }

    pub async fn remove_known_abot(&self, abot_name: &str) -> Result<()> {
        kubo::validate_name(abot_name)?;
        bundle::remove_known_abot(&self.data_dir, abot_name);
        Ok(())
    }

    pub async fn dismiss_variant(&self, abot_name: &str, kubo_name: &str) -> Result<()> {
        self.variant_op(abot_name, kubo_name, VariantOp::Dismiss)
            .await
    }

    pub async fn integrate_variant(&self, abot_name: &str, kubo_name: &str) -> Result<()> {
        self.variant_op(abot_name, kubo_name, VariantOp::Integrate)
            .await
    }

    pub async fn discard_variant(&self, abot_name: &str, kubo_name: &str) -> Result<()> {
        self.variant_op(abot_name, kubo_name, VariantOp::Discard)
            .await
    }

    // ── Internal ────────────────────────────────────────────

    async fn build_session_keys(&self) -> HashSet<String> {
        let sessions = self.sessions.lock().await;
        sessions.keys().cloned().collect()
    }

    async fn variant_op(&self, abot: &str, kubo: &str, op: VariantOp) -> Result<()> {
        kubo::validate_name(abot)?;
        kubo::validate_name(kubo)?;
        let abots_dir = bundle::resolve_abots_dir(&self.data_dir);
        let canonical_path = abots_dir.join(format!("{abot}.abot"));
        let kubo_branch = format!("kubo/{kubo}");

        self.close_session_in_kubo(abot, kubo).await;

        // Kill the tmux session in the container so it doesn't linger.
        {
            let kubos = self.kubos.lock().await;
            if let Some(k) = kubos.get(kubo) {
                if let Some(ref cid) = k.container_id {
                    if let Ok(docker) = bollard::Docker::connect_with_socket_defaults() {
                        let tmux_name = kubo_exec::tmux_session_name(abot);
                        kubo_exec::tmux_kill_session(&docker, cid, &tmux_name).await;
                    }
                }
            }
        }

        match op {
            VariantOp::Dismiss => bundle::dismiss_variant(&canonical_path, &kubo_branch)?,
            VariantOp::Integrate => bundle::integrate_variant(&canonical_path, &kubo_branch)?,
            VariantOp::Discard => bundle::discard_variant(&canonical_path, &kubo_branch)?,
        }

        self.remove_abot_from_kubo_manifest(kubo, abot).await;
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum VariantOp {
    Dismiss,
    Integrate,
    Discard,
}

/// Set `has_session` on each kubo branch entry.
fn inject_has_session(detail: &mut bundle::AbotDetail, session_keys: &HashSet<String>) {
    for branch in &mut detail.kubo_branches {
        let qualified = format!("{}@{}", detail.name, branch.kubo_name);
        branch.has_session = Some(session_keys.contains(&qualified));
    }
}
