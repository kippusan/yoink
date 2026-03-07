use uuid::Uuid;

use crate::{
    db,
    error::{AppError, AppResult},
    services,
    state::AppState,
};

use super::helpers;

pub(super) async fn accept_match_suggestion(
    state: &AppState,
    suggestion_id: Uuid,
) -> AppResult<()> {
    let suggestion = db::load_match_suggestion_by_id(&state.db, suggestion_id)
        .await?
        .ok_or_else(|| AppError::not_found("match suggestion", Some(suggestion_id.to_string())))?;

    match suggestion.scope_type.as_str() {
        "album" => {
            let album_links = db::load_album_provider_links(&state.db, suggestion.scope_id).await?;
            let linked: std::collections::HashSet<(String, String)> = album_links
                .iter()
                .map(|l| (l.provider.clone(), l.external_id.clone()))
                .collect();
            let left_linked = linked.contains(&(
                suggestion.left_provider.clone(),
                suggestion.left_external_id.clone(),
            ));
            let right_linked = linked.contains(&(
                suggestion.right_provider.clone(),
                suggestion.right_external_id.clone(),
            ));
            let (target_provider, target_external_id, target_url) = if left_linked && !right_linked
            {
                (
                    suggestion.right_provider.clone(),
                    suggestion.right_external_id.clone(),
                    suggestion.external_url.clone(),
                )
            } else if right_linked && !left_linked {
                (
                    suggestion.left_provider.clone(),
                    suggestion.left_external_id.clone(),
                    None,
                )
            } else {
                (
                    suggestion.right_provider.clone(),
                    suggestion.right_external_id.clone(),
                    suggestion.external_url.clone(),
                )
            };

            let existing =
                db::find_album_by_provider_link(&state.db, &target_provider, &target_external_id)
                    .await?;

            if let Some(existing_album_id) = existing
                && existing_album_id != suggestion.scope_id
            {
                return Err(AppError::conflict(
                    "provider album is already linked to another local album",
                ));
            }

            let link = db::AlbumProviderLink {
                id: Uuid::now_v7(),
                album_id: suggestion.scope_id,
                provider: target_provider,
                external_id: target_external_id,
                external_url: target_url,
                external_title: suggestion.external_name.clone(),
                cover_ref: None,
            };
            db::upsert_album_provider_link(&state.db, &link).await?;
        }
        "artist" => {
            let artist_links =
                db::load_artist_provider_links(&state.db, suggestion.scope_id).await?;
            let linked: std::collections::HashSet<(String, String)> = artist_links
                .iter()
                .map(|l| (l.provider.clone(), l.external_id.clone()))
                .collect();
            let left_linked = linked.contains(&(
                suggestion.left_provider.clone(),
                suggestion.left_external_id.clone(),
            ));
            let right_linked = linked.contains(&(
                suggestion.right_provider.clone(),
                suggestion.right_external_id.clone(),
            ));
            let (target_provider, target_external_id, target_url) = if left_linked && !right_linked
            {
                (
                    suggestion.right_provider.clone(),
                    suggestion.right_external_id.clone(),
                    suggestion.external_url.clone(),
                )
            } else if right_linked && !left_linked {
                (
                    suggestion.left_provider.clone(),
                    suggestion.left_external_id.clone(),
                    None,
                )
            } else {
                (
                    suggestion.right_provider.clone(),
                    suggestion.right_external_id.clone(),
                    suggestion.external_url.clone(),
                )
            };

            let existing =
                db::find_artist_by_provider_link(&state.db, &target_provider, &target_external_id)
                    .await?;

            if let Some(existing_artist_id) = existing
                && existing_artist_id != suggestion.scope_id
            {
                return Err(AppError::conflict(
                    "provider artist is already linked to another local artist",
                ));
            }

            let link = db::ArtistProviderLink {
                id: Uuid::now_v7(),
                artist_id: suggestion.scope_id,
                provider: target_provider,
                external_id: target_external_id,
                external_url: target_url,
                external_name: suggestion.external_name.clone(),
                image_ref: None,
            };
            db::upsert_artist_provider_link(&state.db, &link).await?;

            services::sync_artist_albums(state, suggestion.scope_id).await?;
            helpers::spawn_recompute_artist_match_suggestions(state, suggestion.scope_id);
        }
        _ => {
            return Err(AppError::validation(
                Some("scope_type"),
                "unknown match suggestion scope type",
            ));
        }
    }

    db::set_match_suggestion_status(&state.db, suggestion_id, "accepted").await?;

    if suggestion.scope_type == "album" {
        let artist_id = {
            let albums = state.monitored_albums.read().await;
            albums
                .iter()
                .find(|a| a.id == suggestion.scope_id)
                .map(|a| a.artist_id)
        };
        if let Some(artist_id) = artist_id {
            helpers::spawn_recompute_artist_match_suggestions(state, artist_id);
        }
    }

    state.notify_sse();
    Ok(())
}

pub(super) async fn dismiss_match_suggestion(
    state: &AppState,
    suggestion_id: Uuid,
) -> AppResult<()> {
    let scope = db::load_match_suggestion_by_id(&state.db, suggestion_id)
        .await
        .ok()
        .flatten();
    db::set_match_suggestion_status(&state.db, suggestion_id, "dismissed").await?;

    if let Some(suggestion) = scope
        && suggestion.scope_type == "album"
    {
        let artist_id = {
            let albums = state.monitored_albums.read().await;
            albums
                .iter()
                .find(|a| a.id == suggestion.scope_id)
                .map(|a| a.artist_id)
        };
        if let Some(artist_id) = artist_id {
            helpers::spawn_recompute_artist_match_suggestions(state, artist_id);
        }
    }
    state.notify_sse();
    Ok(())
}

pub(super) async fn refresh_match_suggestions(state: &AppState, artist_id: Uuid) -> AppResult<()> {
    services::recompute_artist_match_suggestions(state, artist_id).await?;
    state.notify_sse();
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::db;
    use crate::test_helpers::*;

    #[tokio::test]
    async fn dismiss_match_suggestion() {
        let (state, _tmp) = test_app_state().await;
        let artist = seed_artist(&state.db, "Artist").await;
        state.monitored_artists.write().await.push(artist.clone());

        let suggestion = db::MatchSuggestion {
            id: uuid::Uuid::now_v7(),
            scope_type: "artist".to_string(),
            scope_id: artist.id,
            left_provider: "tidal".to_string(),
            left_external_id: "T1".to_string(),
            right_provider: "deezer".to_string(),
            right_external_id: "D1".to_string(),
            match_kind: "name_match".to_string(),
            confidence: 80,
            explanation: None,
            external_name: None,
            external_url: None,
            image_ref: None,
            disambiguation: None,
            artist_type: None,
            country: None,
            tags: vec![],
            popularity: None,
            status: "pending".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        db::upsert_match_suggestion(&state.db, &suggestion)
            .await
            .unwrap();

        super::dismiss_match_suggestion(&state, suggestion.id)
            .await
            .unwrap();

        let loaded = db::load_match_suggestion_by_id(&state.db, suggestion.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.status, "dismissed");
    }
}
