//! Personal access tokens: the bearer credential a user mints for a script, per
//! [authentication.md](../docs/database/authentication.md#personal_access_tokens).
//!
//! The plaintext exists exactly twice: in the create response and in the caller's hands.
//! Here it is hashed on the way in and compared by hash on the way back.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::maps::{Actor, MapError, Result};

/// Marks a token as ours when one turns up in a log or a config file.
const PREFIX: &str = "wst_";

/// A token as the account settings list it: everything except the secret.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct PersonalAccessToken {
    pub id: i64,
    pub name: String,
    #[ts(optional)]
    pub last_used_at: Option<DateTime<Utc>>,
    #[ts(optional)]
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct CreateToken {
    pub name: String,
    /// Absent lasts until revoked.
    #[serde(default)]
    #[ts(optional)]
    pub expires_in_days: Option<i64>,
}

/// The furthest out an expiry may be. Further than that is "until revoked", which is
/// what leaving it out already says, and an unbounded number overflows the date.
const MAX_EXPIRY_DAYS: i64 = 3650;

impl CreateToken {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(MapError::Validation("a token needs a name".into()));
        }
        if self.name.chars().count() > 255 {
            return Err(MapError::Validation("name is too long".into()));
        }
        if self.expires_in_days.is_some_and(|days| days <= 0) {
            return Err(MapError::Validation("expiry must be in the future".into()));
        }
        if self
            .expires_in_days
            .is_some_and(|days| days > MAX_EXPIRY_DAYS)
        {
            return Err(MapError::Validation(
                "expiry can be at most ten years away; leave it out for a token that lasts until revoked"
                    .into(),
            ));
        }
        Ok(())
    }
}

/// The one response that carries the secret.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, utoipa::ToSchema)]
#[ts(export)]
pub struct CreatedToken {
    pub token: PersonalAccessToken,
    pub plaintext: String,
}

/// Two v4 UUIDs: 244 random bits, well past what the spec asks for, from a generator the
/// crate already trusts for session ids.
pub fn generate() -> String {
    format!(
        "{PREFIX}{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

pub fn hash(plaintext: &str) -> String {
    hex::encode(Sha256::digest(plaintext.as_bytes()))
}

pub async fn list(pool: &PgPool, user_id: i64) -> Result<Vec<PersonalAccessToken>> {
    let rows = sqlx::query_as!(
        PersonalAccessToken,
        "select id, name, last_used_at, expires_at, created_at
         from personal_access_tokens where user_id = $1
         order by created_at desc, id desc",
        user_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create(pool: &PgPool, user_id: i64, cmd: CreateToken) -> Result<CreatedToken> {
    cmd.validate()?;
    let plaintext = generate();
    let expires_at = match cmd.expires_in_days {
        Some(days) => Some(
            chrono::TimeDelta::try_days(days)
                .and_then(|delta| Utc::now().checked_add_signed(delta))
                .ok_or_else(|| MapError::Validation("expiry is too far away".into()))?,
        ),
        None => None,
    };
    let token = sqlx::query_as!(
        PersonalAccessToken,
        "insert into personal_access_tokens (user_id, name, token_hash, expires_at)
         values ($1, $2, $3, $4)
         returning id, name, last_used_at, expires_at, created_at",
        user_id,
        cmd.name.trim(),
        hash(&plaintext),
        expires_at,
    )
    .fetch_one(pool)
    .await?;
    Ok(CreatedToken { token, plaintext })
}

/// `NotFound` for a token that is not this user's, so revoking is never a way to find out
/// whether somebody else's id exists.
pub async fn revoke(pool: &PgPool, user_id: i64, token_id: i64) -> Result<()> {
    let deleted = sqlx::query!(
        "delete from personal_access_tokens where id = $1 and user_id = $2",
        token_id,
        user_id,
    )
    .execute(pool)
    .await?
    .rows_affected();
    if deleted == 0 {
        return Err(MapError::NotFound);
    }
    Ok(())
}

/// Who a bearer token acts as: the owning user, flying their preferred character. `None`
/// for a token that is unknown, expired, or belongs to an account with no characters
/// left, which are all the same thing to the caller.
///
/// `last_used_at` moves at most every five minutes. It answers "is this token still in
/// use", and a script polling a map every few seconds would otherwise write a row each
/// time, the way sessions renew at most daily for the same reason.
pub async fn actor_for_token(pool: &PgPool, plaintext: &str) -> Result<Option<Actor>> {
    let row = sqlx::query!(
        r#"with live as (
               select t.id, t.user_id, t.last_used_at,
                      (select id from characters
                       where user_id = t.user_id
                       order by is_preferred desc, id
                       limit 1) as character_id
               from personal_access_tokens t
               where t.token_hash = $1
                 and (t.expires_at is null or t.expires_at > now())
           ), touched as (
               update personal_access_tokens set last_used_at = now()
               where id = (select id from live
                           where last_used_at is null
                              or last_used_at < now() - interval '5 minutes')
           )
           select user_id as "user_id!", character_id as "character_id!"
           from live where character_id is not null"#,
        hash(plaintext),
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| Actor {
        user_id: r.user_id,
        character_id: r.character_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_prefixed_random_and_never_repeats() {
        let a = generate();
        let b = generate();
        assert!(a.starts_with(PREFIX));
        assert_ne!(a, b);
        assert_eq!(a.len(), PREFIX.len() + 64);
    }

    #[test]
    fn the_hash_is_stable_and_not_the_plaintext() {
        let plaintext = generate();
        assert_eq!(hash(&plaintext), hash(&plaintext));
        assert_ne!(hash(&plaintext), plaintext);
        assert_eq!(hash(&plaintext).len(), 64);
        assert_ne!(hash(&plaintext), hash(&generate()));
    }

    #[test]
    fn a_blank_name_or_a_past_expiry_is_refused() {
        let blank = CreateToken {
            name: "   ".into(),
            expires_in_days: None,
        };
        assert!(blank.validate().is_err());
        let past = CreateToken {
            name: "script".into(),
            expires_in_days: Some(0),
        };
        assert!(past.validate().is_err());
        let fine = CreateToken {
            name: "script".into(),
            expires_in_days: Some(30),
        };
        assert!(fine.validate().is_ok());
        // Past this the date arithmetic would overflow and panic.
        let absurd = CreateToken {
            name: "script".into(),
            expires_in_days: Some(i64::MAX),
        };
        assert!(absurd.validate().is_err());
        let ten_years = CreateToken {
            name: "script".into(),
            expires_in_days: Some(MAX_EXPIRY_DAYS),
        };
        assert!(ten_years.validate().is_ok());
    }
}
