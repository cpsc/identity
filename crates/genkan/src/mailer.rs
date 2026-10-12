//! Mailing invitation links, when `genkan` is given an SMTP submission server.

use std::time::Duration;

use genkan_core::clock::{Clock, UnixTime};
use genkan_core::config::BaseUrl;
use genkan_core::crypto::hash_secret;
use genkan_k8s::crd::invitation::{Invitation, InvitationStatus};
use genkan_k8s::invitations::{InviteError, link, new_token};
use genkan_k8s::objects::{ReplaceOutcome, replace_status_if_unchanged};
use genkan_mail::{MailError, Mailer};
use genkan_ui::i18n::Lang;
use kube::ResourceExt as _;
use kube::api::{Api, ListParams};
use thiserror::Error;
use tokio::sync::watch;

/// The subject and body of an invitation mail.
#[must_use]
pub fn mail_text(language: Option<&str>, link: &str, expires_in: Duration) -> (String, String) {
    let lang = language.and_then(Lang::from_range).unwrap_or(Lang::En);
    let m = lang.messages();
    let days = expires_in.as_secs().div_ceil(86_400).max(1);
    let body = m
        .mail_body
        .replace("%LINK%", link)
        .replace("%DAYS%", &days.to_string());
    (m.mail_subject.to_owned(), body)
}

/// How often pending invitations are looked for.
const MAIL_INTERVAL: Duration = Duration::from_secs(30);

/// Mails invitation links until `shutdown`.
///
/// For each unexpired invitation with an `email` and no token yet, it generates a token, records
/// its hash in `status` with a compare-and-swap (so only one process ever sends a link for an
/// invitation), then sends the mail and records `sentAt`. A link whose sending failed is not
/// retried, because only its hash is kept: the admin sees `tokenHash` without `sentAt` and creates
/// a new invitation.
pub async fn run_mailer<C: Clock>(
    api: Api<Invitation>,
    mailer: Mailer,
    issuer: BaseUrl,
    clock: C,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(MAIL_INTERVAL);
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = shutdown.changed() => return,
        }
        let pending = match api.list(&ListParams::default()).await {
            Ok(list) => list.items,
            Err(error) => {
                tracing::warn!(%error, "listing invitations failed");
                continue;
            }
        };
        for invitation in pending {
            if let Err(error) = mail_one(&api, &mailer, &issuer, &clock, invitation).await {
                tracing::error!(%error, "mailing an invitation failed");
            }
        }
    }
}

/// Why one invitation could not be mailed.
#[derive(Debug, Error)]
enum MailOneError {
    #[error("Kubernetes API: {0}")]
    Kubernetes(#[from] kube::Error),
    #[error(transparent)]
    Invite(#[from] InviteError),
    #[error(transparent)]
    Mail(#[from] MailError),
}

async fn mail_one<C: Clock>(
    api: &Api<Invitation>,
    mailer: &Mailer,
    issuer: &BaseUrl,
    clock: &C,
    invitation: Invitation,
) -> Result<(), MailOneError> {
    let now = clock.now();
    let expires_at = UnixTime::from_secs(u64::try_from(invitation.spec.expires_at).unwrap_or(0));
    let has_token = invitation.spec.token_hash.is_some()
        || invitation
            .status
            .as_ref()
            .is_some_and(|s| s.token_hash.is_some());
    let Some(email) = invitation.spec.email.clone() else {
        return Ok(());
    };
    if has_token || expires_at <= now {
        return Ok(());
    }
    let name = invitation.name_any();
    let token = new_token(&name)?;
    let mut claimed = invitation.clone();
    claimed.status = Some(InvitationStatus {
        token_hash: Some(hash_secret(token.as_bytes())),
        sent_at: None,
    });
    let claimed = match replace_status_if_unchanged(api, &claimed).await? {
        ReplaceOutcome::Replaced(object) => object,
        // Someone else claimed it, or it was redeemed or deleted meanwhile.
        ReplaceOutcome::Conflict | ReplaceOutcome::NotFound => return Ok(()),
    };
    let (subject, body) = mail_text(
        invitation.spec.language.as_deref(),
        &link(issuer, &token),
        Duration::from_secs(expires_at.as_secs().saturating_sub(now.as_secs())),
    );
    mailer.send(&email, &subject, &body, now).await?;
    tracing::info!(invitation = %name, user = %invitation.spec.user, "invitation mailed");
    let mut sent = claimed;
    if let Some(status) = sent.status.as_mut() {
        status.sent_at = i64::try_from(now.as_secs()).ok();
    }
    // Best effort: the link works whether or not `sentAt` is recorded.
    if let Err(error) = replace_status_if_unchanged(api, &sent).await {
        tracing::warn!(%error, invitation = %name, "recording sentAt failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_text_is_localised_and_carries_the_link() {
        let (subject, body) = mail_text(
            Some("zh-TW"),
            "https://login.example.com/setup#token=inv-1.x",
            Duration::from_hours(36),
        );
        assert_eq!(subject, "設定你的通行密鑰");
        assert!(body.contains("https://login.example.com/setup#token=inv-1.x"));
        assert!(body.contains("2 天"));
        let (subject, body) = mail_text(None, "L", Duration::from_secs(1));
        assert_eq!(subject, "Set up your passkey");
        assert!(body.contains("1 day(s)"));
    }
}
