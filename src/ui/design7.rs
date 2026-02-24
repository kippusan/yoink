use std::collections::HashMap;

use leptos::prelude::*;

use super::{
    album_cover_url, album_glow_script, album_profile_url, artist_image_url, artist_profile_url,
    build_albums_by_artist, build_artist_names, build_latest_jobs, instant_search_script,
    live_updates_script, monitored_artist_image_url, monitored_artist_profile_url, status_class,
    theme_bootstrap_script, theme_interaction_script, tracklist_script,
};
use crate::{config::QUALITY_WARNING, models::*};

// ── Custom CSS — "Glass Dark" glassmorphism aesthetic ────────

fn custom_css() -> &'static str {
    r#"
/* ── Reset & base ─────────────────────────────────────────── */
*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

.d7-body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  font-size: 15px;
  line-height: 1.5;
  background: #f4f4f5;
  color: #18181b;
  min-height: 100vh;
  -webkit-font-smoothing: antialiased;
}
.dark .d7-body {
  background: #09090b;
  color: #f4f4f5;
}

/* ── Layout shell ─────────────────────────────────────────── */
.d7-wrapper {
  display: flex;
  min-height: 100vh;
}

/* ── Sidebar ──────────────────────────────────────────────── */
.d7-sidebar {
  position: fixed;
  top: 0;
  left: 0;
  bottom: 0;
  width: 220px;
  background: rgba(10, 10, 15, 0.92);
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  border-right: 1px solid rgba(255, 255, 255, 0.06);
  display: flex;
  flex-direction: column;
  z-index: 50;
  overflow-y: auto;
}

.d7-sidebar-brand {
  padding: 20px 16px 12px;
  display: flex;
  align-items: center;
  gap: 10px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.06);
}

.d7-sidebar-brand-icon {
  width: 32px;
  height: 32px;
  border-radius: 8px;
  background: linear-gradient(135deg, #3b82f6, #60a5fa);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  box-shadow: 0 0 16px rgba(59, 130, 246, 0.3);
}

.d7-sidebar-brand-icon svg {
  width: 18px;
  height: 18px;
  fill: white;
}

.d7-sidebar-brand-text {
  font-size: 18px;
  font-weight: 700;
  color: #f4f4f5;
  letter-spacing: 0.5px;
}

.d7-sidebar-nav {
  flex: 1;
  padding: 8px 0;
}

.d7-nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  color: rgba(161, 161, 170, 0.9);
  text-decoration: none;
  font-size: 14px;
  font-weight: 500;
  border-left: 3px solid transparent;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.d7-nav-item:hover {
  background: rgba(255, 255, 255, 0.04);
  color: #e4e4e7;
}
.d7-nav-item.active {
  border-left-color: #3b82f6;
  color: #f4f4f5;
  background: rgba(59, 130, 246, 0.08);
}
.d7-nav-item svg {
  width: 18px;
  height: 18px;
  flex-shrink: 0;
}

.d7-sidebar-footer {
  padding: 12px 16px;
  border-top: 1px solid rgba(255, 255, 255, 0.06);
}

.d7-theme-btn {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  background: none;
  border: none;
  color: rgba(161, 161, 170, 0.9);
  font-family: inherit;
  font-size: 13px;
  cursor: pointer;
  padding: 8px 4px;
  border-radius: 6px;
  transition: background 0.15s, color 0.15s;
}
.d7-theme-btn:hover {
  background: rgba(255, 255, 255, 0.04);
  color: #e4e4e7;
}
.d7-theme-btn svg {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}

/* ── Main content ─────────────────────────────────────────── */
.d7-content {
  margin-left: 220px;
  flex: 1;
  min-height: 100vh;
}

.d7-topbar {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
  padding: 14px 24px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  position: sticky;
  top: 0;
  z-index: 40;
}
.dark .d7-topbar {
  background: rgba(24, 24, 27, 0.6);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border-bottom-color: rgba(255, 255, 255, 0.06);
}

.d7-topbar-title {
  font-size: 18px;
  font-weight: 600;
  color: #18181b;
  margin: 0;
}
.dark .d7-topbar-title {
  color: #f4f4f5;
}

.d7-main {
  padding: 24px;
}

/* ── Glass panel (generic container) ──────────────────────── */
.d7-glass {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border: 1px solid rgba(0, 0, 0, 0.06);
  border-radius: 12px;
  margin-bottom: 24px;
  overflow: hidden;
}
.dark .d7-glass {
  background: rgba(24, 24, 27, 0.6);
  border-color: rgba(255, 255, 255, 0.08);
}

.d7-glass-header {
  padding: 14px 20px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.dark .d7-glass-header {
  border-bottom-color: rgba(255, 255, 255, 0.06);
}

.d7-glass-title {
  font-size: 15px;
  font-weight: 600;
  color: #18181b;
  margin: 0;
}
.dark .d7-glass-title {
  color: #f4f4f5;
}

.d7-glass-body {
  padding: 16px 20px;
}

/* ── Stat cards ───────────────────────────────────────────── */
.d7-stats {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 16px;
  margin-bottom: 24px;
}

.d7-stat-card {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border: 1px solid rgba(0, 0, 0, 0.06);
  border-radius: 12px;
  padding: 16px;
  position: relative;
  overflow: hidden;
  transition: transform 0.15s, box-shadow 0.15s;
}
.d7-stat-card:hover {
  transform: translateY(-1px);
  box-shadow: 0 4px 24px rgba(59, 130, 246, 0.08);
}
.dark .d7-stat-card {
  background: rgba(24, 24, 27, 0.6);
  border-color: rgba(255, 255, 255, 0.08);
}
.dark .d7-stat-card:hover {
  box-shadow: 0 4px 24px rgba(59, 130, 246, 0.12);
}

/* Blue glow top border */
.d7-stat-card::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 2px;
  background: linear-gradient(90deg, #3b82f6, #60a5fa);
  box-shadow: 0 0 12px rgba(59, 130, 246, 0.4);
}

.d7-stat-label {
  font-size: 12px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: #71717a;
  margin: 0 0 4px;
}
.dark .d7-stat-label {
  color: #a1a1aa;
}

.d7-stat-value {
  font-size: 28px;
  font-weight: 700;
  color: #18181b;
  margin: 0;
}
.dark .d7-stat-value {
  color: #f4f4f5;
}

/* ── Artist crate cards ───────────────────────────────────── */
.d7-artist-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 16px;
}

.d7-artist-card {
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border: 1px solid rgba(0, 0, 0, 0.06);
  border-radius: 12px;
  padding: 16px;
  display: flex;
  align-items: center;
  gap: 14px;
  transition: transform 0.2s, box-shadow 0.2s, border-color 0.2s;
  position: relative;
  overflow: hidden;
}
.d7-artist-card:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 32px rgba(59, 130, 246, 0.1);
  border-color: rgba(59, 130, 246, 0.2);
}
.dark .d7-artist-card {
  background: rgba(24, 24, 27, 0.6);
  border-color: rgba(255, 255, 255, 0.08);
}
.dark .d7-artist-card:hover {
  box-shadow: 0 8px 32px rgba(59, 130, 246, 0.15);
  border-color: rgba(59, 130, 246, 0.3);
}

.d7-artist-avatar {
  width: 48px;
  height: 48px;
  border-radius: 50%;
  object-fit: cover;
  border: 2px solid rgba(59, 130, 246, 0.2);
  flex-shrink: 0;
  background: #e4e4e7;
}
.dark .d7-artist-avatar {
  border-color: rgba(59, 130, 246, 0.3);
  background: #27272a;
}

.d7-artist-fallback {
  width: 48px;
  height: 48px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: #e4e4e7;
  color: #71717a;
  font-weight: 700;
  font-size: 18px;
  border: 2px solid rgba(59, 130, 246, 0.2);
  flex-shrink: 0;
}
.dark .d7-artist-fallback {
  background: #27272a;
  color: #a1a1aa;
  border-color: rgba(59, 130, 246, 0.3);
}

.d7-artist-info {
  flex: 1;
  min-width: 0;
}

.d7-artist-name {
  font-size: 15px;
  font-weight: 700;
  color: #18181b;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dark .d7-artist-name {
  color: #f4f4f5;
}

.d7-artist-meta {
  font-size: 12px;
  color: #71717a;
}
.dark .d7-artist-meta {
  color: #a1a1aa;
}

.d7-artist-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}

/* ── Album sleeve cards (record sleeve inspired) ──────────── */
.d7-album-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 20px;
}

.d7-sleeve {
  --d7-glow-rgb: 59, 130, 246;
  position: relative;
  background: rgba(255, 255, 255, 0.7);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border: 1px solid rgba(0, 0, 0, 0.06);
  border-radius: 12px;
  overflow: hidden;
  transition: transform 0.2s, box-shadow 0.2s, border-color 0.2s;
  cursor: default;
}
.d7-sleeve:hover {
  transform: translateY(-4px);
  box-shadow: 0 12px 40px rgba(var(--d7-glow-rgb), 0.18);
  border-color: rgba(var(--d7-glow-rgb), 0.38);
}
.dark .d7-sleeve {
  background: rgba(24, 24, 27, 0.6);
  border-color: rgba(255, 255, 255, 0.08);
}
.dark .d7-sleeve:hover {
  box-shadow: 0 12px 40px rgba(var(--d7-glow-rgb), 0.3);
  border-color: rgba(var(--d7-glow-rgb), 0.5);
}

/* Glow ring on hover around cover */
.d7-sleeve-cover-wrap {
  position: relative;
  width: 100%;
  padding-top: 100%;
  background: #e4e4e7;
  overflow: hidden;
}
.dark .d7-sleeve-cover-wrap {
  background: #27272a;
}
.d7-sleeve:hover .d7-sleeve-cover-wrap::after {
  content: '';
  position: absolute;
  inset: 0;
  border: 2px solid rgba(var(--d7-glow-rgb), 0.55);
  border-radius: 0;
  pointer-events: none;
  box-shadow: inset 0 0 20px rgba(var(--d7-glow-rgb), 0.22);
  z-index: 4;
}

.d7-sleeve-cover {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.d7-sleeve-fallback {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 36px;
  font-weight: 800;
  color: #a1a1aa;
}
.dark .d7-sleeve-fallback {
  color: #52525b;
}

/* Badge stickers */
.d7-badge {
  position: absolute;
  z-index: 3;
  font-size: 9px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  padding: 3px 8px;
  border-radius: 6px;
  white-space: nowrap;
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
}
.d7-badge-wanted {
  top: 8px;
  right: 8px;
  background: rgba(var(--d7-glow-rgb), 0.9);
  color: #fff;
  box-shadow: 0 2px 10px rgba(var(--d7-glow-rgb), 0.35);
}
.d7-badge-acquired {
  top: 8px;
  right: 8px;
  background: rgba(var(--d7-glow-rgb), 0.72);
  color: #fff;
  box-shadow: 0 2px 10px rgba(var(--d7-glow-rgb), 0.28);
}
.d7-badge-explicit {
  top: 8px;
  left: 8px;
  background: rgba(63, 63, 70, 0.85);
  color: #e4e4e7;
  font-size: 8px;
  padding: 2px 5px;
}
.dark .d7-badge-explicit {
  background: rgba(161, 161, 170, 0.85);
  color: #18181b;
}

/* Sleeve info panel */
.d7-sleeve-info {
  padding: 10px 12px;
}

.d7-sleeve-title {
  font-size: 13px;
  font-weight: 600;
  color: #18181b;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dark .d7-sleeve-title {
  color: #f4f4f5;
}
.d7-sleeve-title a {
  color: inherit;
  text-decoration: none;
}
.d7-sleeve-title a:hover {
  color: #3b82f6;
}

.d7-sleeve-sub {
  font-size: 11px;
  color: #71717a;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dark .d7-sleeve-sub {
  color: #a1a1aa;
}

.d7-sleeve-status {
  margin-top: 6px;
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

/* Status dot */
.d7-status-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  display: inline-block;
  flex-shrink: 0;
}
.d7-status-dot-blue {
  background: #3b82f6;
  box-shadow: 0 0 6px rgba(59, 130, 246, 0.5);
}
.d7-status-dot-green {
  background: #22c55e;
  box-shadow: 0 0 6px rgba(34, 197, 94, 0.5);
}
.d7-status-dot-zinc {
  background: #71717a;
}

.d7-sleeve-actions {
  margin-top: 8px;
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.d7-sleeve-action-btn {
  padding: 3px 8px;
  font-size: 11px;
  min-height: 26px;
}

/* ── Pill / badge (status pills for tables) ───────────────── */
.pill {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 9999px;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}
.pill.status-queued {
  background: rgba(245, 158, 11, 0.12);
  color: #b45309;
}
.pill.status-downloading {
  background: rgba(59, 130, 246, 0.12);
  color: #2563eb;
}
.pill.status-completed {
  background: rgba(34, 197, 94, 0.12);
  color: #16a34a;
}
.pill.status-failed {
  background: rgba(239, 68, 68, 0.12);
  color: #dc2626;
}
.dark .pill.status-queued {
  background: rgba(245, 158, 11, 0.15);
  color: #fbbf24;
}
.dark .pill.status-downloading {
  background: rgba(59, 130, 246, 0.15);
  color: #60a5fa;
}
.dark .pill.status-completed {
  background: rgba(34, 197, 94, 0.15);
  color: #4ade80;
}
.dark .pill.status-failed {
  background: rgba(239, 68, 68, 0.15);
  color: #f87171;
}

.d7-pill-muted {
  background: rgba(0, 0, 0, 0.05);
  color: #71717a;
}
.dark .d7-pill-muted {
  background: rgba(255, 255, 255, 0.06);
  color: #a1a1aa;
}

/* ── Table (for dashboard activity) ───────────────────────── */
.d7-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}
.d7-table th {
  text-align: left;
  padding: 10px 12px;
  font-weight: 600;
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: #71717a;
  background: rgba(0, 0, 0, 0.02);
  border-bottom: 1px solid rgba(0, 0, 0, 0.06);
  white-space: nowrap;
}
.dark .d7-table th {
  color: #a1a1aa;
  background: rgba(255, 255, 255, 0.02);
  border-bottom-color: rgba(255, 255, 255, 0.06);
}
.d7-table td {
  padding: 8px 12px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.04);
  color: #3f3f46;
  vertical-align: middle;
}
.dark .d7-table td {
  border-bottom-color: rgba(255, 255, 255, 0.04);
  color: #d4d4d8;
}
.d7-table tbody tr:hover {
  background: rgba(59, 130, 246, 0.03);
}
.dark .d7-table tbody tr:hover {
  background: rgba(59, 130, 246, 0.05);
}
.d7-table tbody tr:last-child td {
  border-bottom: none;
}

/* ── Buttons ──────────────────────────────────────────────── */
.d7-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 14px;
  background: rgba(255, 255, 255, 0.6);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  font-family: inherit;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  color: #3f3f46;
  text-decoration: none;
  transition: all 0.15s;
  white-space: nowrap;
}
.d7-btn:hover {
  background: rgba(255, 255, 255, 0.85);
  border-color: rgba(59, 130, 246, 0.2);
}
.dark .d7-btn {
  background: rgba(39, 39, 42, 0.6);
  border-color: rgba(255, 255, 255, 0.1);
  color: #d4d4d8;
}
.dark .d7-btn:hover {
  background: rgba(39, 39, 42, 0.85);
  border-color: rgba(59, 130, 246, 0.3);
}

.d7-btn-primary {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
  box-shadow: 0 2px 12px rgba(59, 130, 246, 0.25);
}
.d7-btn-primary:hover {
  background: #60a5fa;
  border-color: #60a5fa;
  box-shadow: 0 4px 20px rgba(59, 130, 246, 0.35);
}
.dark .d7-btn-primary {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
}
.dark .d7-btn-primary:hover {
  background: #60a5fa;
  border-color: #60a5fa;
}

.d7-btn-sm {
  padding: 3px 10px;
  font-size: 12px;
}

.d7-btn-danger {
  background: rgba(239, 68, 68, 0.08);
  border-color: rgba(239, 68, 68, 0.3);
  color: #dc2626;
}
.d7-btn-danger:hover {
  background: rgba(239, 68, 68, 0.15);
  border-color: #dc2626;
}
.dark .d7-btn-danger {
  background: rgba(239, 68, 68, 0.1);
  border-color: rgba(248, 113, 113, 0.3);
  color: #f87171;
}
.dark .d7-btn-danger:hover {
  background: rgba(239, 68, 68, 0.2);
  border-color: #f87171;
}

/* Small icon button (for sleeve actions) */
.d7-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.5);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  color: #71717a;
  cursor: pointer;
  transition: all 0.15s;
  padding: 0;
  font-family: inherit;
  font-size: 13px;
}
.d7-icon-btn:hover {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
  box-shadow: 0 2px 8px rgba(59, 130, 246, 0.3);
}
.dark .d7-icon-btn {
  border-color: rgba(255, 255, 255, 0.1);
  background: rgba(39, 39, 42, 0.5);
  color: #a1a1aa;
}
.dark .d7-icon-btn:hover {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
}

/* ── Search input ─────────────────────────────────────────── */
.d7-search-input {
  padding: 8px 14px;
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  font-family: inherit;
  font-size: 14px;
  background: rgba(255, 255, 255, 0.6);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  color: #18181b;
  outline: none;
  width: 100%;
  max-width: 360px;
  transition: border-color 0.15s, box-shadow 0.15s;
}
.d7-search-input:focus {
  border-color: #3b82f6;
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.15);
}
.d7-search-input::placeholder {
  color: #a1a1aa;
}
.dark .d7-search-input {
  background: rgba(39, 39, 42, 0.6);
  border-color: rgba(255, 255, 255, 0.1);
  color: #f4f4f5;
}
.dark .d7-search-input:focus {
  border-color: #3b82f6;
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.2);
}
.dark .d7-search-input::placeholder {
  color: #52525b;
}

/* ── Search results ───────────────────────────────────────── */
.d7-search-result {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 12px 16px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.04);
  transition: background 0.12s;
}
.d7-search-result:last-child {
  border-bottom: none;
}
.d7-search-result:hover {
  background: rgba(59, 130, 246, 0.04);
}
.dark .d7-search-result {
  border-bottom-color: rgba(255, 255, 255, 0.04);
}
.dark .d7-search-result:hover {
  background: rgba(59, 130, 246, 0.06);
}
.d7-search-result-info {
  flex: 1;
  min-width: 0;
}
.d7-search-result-name {
  font-size: 15px;
  font-weight: 600;
  color: #18181b;
}
.dark .d7-search-result-name {
  color: #f4f4f5;
}

/* ── Wanted item row ──────────────────────────────────────── */
.d7-wanted-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 14px 20px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.04);
  transition: background 0.12s;
}
.d7-wanted-card:last-child {
  border-bottom: none;
}
.d7-wanted-card:hover {
  background: rgba(59, 130, 246, 0.03);
}
.dark .d7-wanted-card {
  border-bottom-color: rgba(255, 255, 255, 0.04);
}
.dark .d7-wanted-card:hover {
  background: rgba(59, 130, 246, 0.05);
}

.d7-wanted-thumb {
  width: 48px;
  height: 48px;
  border-radius: 8px;
  object-fit: cover;
  flex-shrink: 0;
  background: #e4e4e7;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
}
.dark .d7-wanted-thumb {
  background: #27272a;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
}
.d7-wanted-thumb-fallback {
  width: 48px;
  height: 48px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: #e4e4e7;
  color: #a1a1aa;
  font-weight: 700;
  font-size: 16px;
  flex-shrink: 0;
}
.dark .d7-wanted-thumb-fallback {
  background: #27272a;
  color: #52525b;
}

.d7-wanted-info {
  flex: 1;
  min-width: 0;
}
.d7-wanted-title {
  font-size: 14px;
  font-weight: 600;
  color: #18181b;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dark .d7-wanted-title {
  color: #f4f4f5;
}
.d7-wanted-title a {
  color: inherit;
  text-decoration: none;
}
.d7-wanted-title a:hover {
  color: #3b82f6;
}
.d7-wanted-meta {
  font-size: 12px;
  color: #71717a;
}
.dark .d7-wanted-meta {
  color: #a1a1aa;
}
.d7-wanted-error {
  font-size: 11px;
  color: #dc2626;
  margin-top: 2px;
}
.dark .d7-wanted-error {
  color: #f87171;
}
.d7-wanted-actions {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
  align-items: center;
}

/* ── Group header (wanted by artist) ──────────────────────── */
.d7-group-header {
  font-size: 13px;
  font-weight: 700;
  color: #3b82f6;
  padding: 10px 20px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.04);
  background: rgba(59, 130, 246, 0.03);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.dark .d7-group-header {
  color: #60a5fa;
  border-bottom-color: rgba(255, 255, 255, 0.04);
  background: rgba(59, 130, 246, 0.05);
}

/* ── Warning banner ───────────────────────────────────────── */
.d7-warning {
  padding: 12px 16px;
  margin-bottom: 24px;
  border-radius: 10px;
  font-size: 13px;
  background: rgba(245, 158, 11, 0.08);
  border: 1px solid rgba(245, 158, 11, 0.2);
  color: #92400e;
}
.dark .d7-warning {
  background: rgba(245, 158, 11, 0.06);
  border-color: rgba(245, 158, 11, 0.15);
  color: #fbbf24;
}

/* ── Muted text ───────────────────────────────────────────── */
.d7-muted {
  color: #71717a;
}
.dark .d7-muted {
  color: #a1a1aa;
}

/* ── Link ─────────────────────────────────────────────────── */
.d7-link {
  color: #3b82f6;
  text-decoration: none;
  font-weight: 500;
}
.d7-link:hover {
  color: #60a5fa;
  text-decoration: underline;
}

/* ── Empty state ──────────────────────────────────────────── */
.d7-empty {
  text-align: center;
  padding: 40px 16px;
  color: #a1a1aa;
  font-size: 14px;
}
.dark .d7-empty {
  color: #52525b;
}

/* ── Tracklist panel ──────────────────────────────────────── */
.d7-tracklist-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.5);
  backdrop-filter: blur(8px);
  -webkit-backdrop-filter: blur(8px);
  color: #71717a;
  cursor: pointer;
  transition: all 0.15s;
  padding: 0;
  font-family: inherit;
  font-size: 11px;
  font-weight: 700;
}
.d7-tracklist-btn:hover {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
  box-shadow: 0 2px 8px rgba(59, 130, 246, 0.3);
}
.dark .d7-tracklist-btn {
  border-color: rgba(255, 255, 255, 0.1);
  background: rgba(39, 39, 42, 0.5);
  color: #a1a1aa;
}
.dark .d7-tracklist-btn:hover {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
}
.d7-tracklist-btn.active {
  background: #3b82f6;
  border-color: #3b82f6;
  color: #fff;
}

.d7-tracklist {
  border-top: 1px solid rgba(0, 0, 0, 0.06);
  max-height: 0;
  overflow: hidden;
  transition: max-height 0.3s ease;
}
.dark .d7-tracklist {
  border-top-color: rgba(255, 255, 255, 0.06);
}
.d7-tracklist.open {
  max-height: 600px;
  overflow-y: auto;
}

.d7-track-row {
  display: flex;
  align-items: center;
  padding: 5px 12px;
  gap: 8px;
  font-size: 12px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.03);
}
.d7-track-row:last-child {
  border-bottom: none;
}
.dark .d7-track-row {
  border-bottom-color: rgba(255, 255, 255, 0.03);
}
.d7-track-num {
  color: #a1a1aa;
  width: 22px;
  text-align: right;
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}
.dark .d7-track-num {
  color: #52525b;
}
.d7-track-title {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: #3f3f46;
}
.dark .d7-track-title {
  color: #d4d4d8;
}
.d7-track-dur {
  color: #a1a1aa;
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}
.dark .d7-track-dur {
  color: #52525b;
}
.d7-tracklist-loading {
  padding: 12px;
  text-align: center;
  font-size: 12px;
  color: #a1a1aa;
}
.dark .d7-tracklist-loading {
  color: #52525b;
}

/* ── Instant search dropdown ─────────────────────────────── */
.d7-search-wrapper {
  position: relative;
  flex: 1;
  max-width: 360px;
  z-index: 2000;
  isolation: isolate;
}
.d7-search-dropdown {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  background: rgba(255, 255, 255, 0.95);
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 10px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.12);
  z-index: 2100;
  max-height: 400px;
  overflow-y: auto;
  display: none;
}
.d7-search-dropdown.visible {
  display: block;
}
.dark .d7-search-dropdown {
  background: rgba(24, 24, 27, 0.95);
  border-color: rgba(255, 255, 255, 0.1);
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
}

.d7-search-dropdown-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-bottom: 1px solid rgba(0, 0, 0, 0.04);
  cursor: default;
  transition: background 0.1s;
}
.d7-search-dropdown-item:last-child {
  border-bottom: none;
}
.d7-search-dropdown-item:hover {
  background: rgba(59, 130, 246, 0.05);
}
.dark .d7-search-dropdown-item {
  border-bottom-color: rgba(255, 255, 255, 0.04);
}
.dark .d7-search-dropdown-item:hover {
  background: rgba(59, 130, 246, 0.08);
}
.d7-search-dropdown-avatar {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  object-fit: cover;
  flex-shrink: 0;
  background: #e4e4e7;
}
.dark .d7-search-dropdown-avatar {
  background: #27272a;
}
.d7-search-dropdown-fallback {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #e4e4e7;
  color: #71717a;
  font-weight: 700;
  font-size: 14px;
  flex-shrink: 0;
}
.dark .d7-search-dropdown-fallback {
  background: #27272a;
  color: #52525b;
}
.d7-search-dropdown-info {
  flex: 1;
  min-width: 0;
}
.d7-search-dropdown-name {
  font-size: 14px;
  font-weight: 600;
  color: #18181b;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.dark .d7-search-dropdown-name {
  color: #f4f4f5;
}
.d7-search-dropdown-hint {
  font-size: 11px;
  color: #a1a1aa;
}
.dark .d7-search-dropdown-hint {
  color: #52525b;
}
.d7-search-dropdown-loading {
  padding: 16px;
  text-align: center;
  font-size: 13px;
  color: #a1a1aa;
}
.dark .d7-search-dropdown-loading {
  color: #52525b;
}

/* ── Utility ──────────────────────────────────────────────── */
.hidden { display: none !important; }

/* ── Responsive ───────────────────────────────────────────── */
@media (max-width: 768px) {
  .d7-sidebar {
    display: none;
  }
  .d7-content {
    margin-left: 0;
  }
  .d7-stats {
    grid-template-columns: repeat(2, 1fr);
  }
  .d7-artist-grid {
    grid-template-columns: 1fr;
  }
  .d7-album-grid {
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 12px;
  }
  .d7-main {
    padding: 16px;
  }
}
"#
}

// ── Layout wrapper ──────────────────────────────────────────

fn layout(title: &str, body: &str, _prefix: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title} - yoink</title><script>{theme_boot}</script><link rel="stylesheet" href="/pkg/yoink.css"><style>{custom_css}</style></head><body class="d7-body">{body}<script>{theme_js}</script><script>{live_js}</script><script>{tracklist_js}</script><script>{search_js}</script><script>{glow_js}</script></body></html>"#,
        title = title,
        theme_boot = theme_bootstrap_script(),
        custom_css = custom_css(),
        theme_js = theme_interaction_script(),
        live_js = live_updates_script(),
        tracklist_js = tracklist_script(),
        search_js = instant_search_script(),
        glow_js = album_glow_script(),
    )
}

// ── SVG icons ───────────────────────────────────────────────

fn icon_house() -> &'static str {
    r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/></svg>"#
}

fn icon_mic() -> &'static str {
    r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" y1="19" x2="12" y2="23"/><line x1="8" y1="23" x2="16" y2="23"/></svg>"#
}

fn icon_heart() -> &'static str {
    r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg>"#
}

fn icon_sun_moon() -> &'static str {
    r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>"#
}

fn icon_music() -> &'static str {
    r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>"#
}

// ── Sidebar component ───────────────────────────────────────

#[component]
fn Sidebar(active: &'static str, prefix: String) -> impl IntoView {
    let dashboard_href = format!("{}/", prefix);
    let artists_href = format!("{}/artists", prefix);
    let wanted_href = format!("{}/wanted", prefix);

    let dashboard_class = if active == "dashboard" {
        "d7-nav-item active"
    } else {
        "d7-nav-item"
    };
    let artists_class = if active == "artists" {
        "d7-nav-item active"
    } else {
        "d7-nav-item"
    };
    let wanted_class = if active == "wanted" {
        "d7-nav-item active"
    } else {
        "d7-nav-item"
    };

    view! {
        <aside class="d7-sidebar">
            <div class="d7-sidebar-brand">
                <div class="d7-sidebar-brand-icon" inner_html=icon_music()></div>
                <span class="d7-sidebar-brand-text">"yoink"</span>
            </div>
            <nav class="d7-sidebar-nav">
                <a href=dashboard_href class=dashboard_class>
                    <span inner_html=icon_house()></span>
                    "Dashboard"
                </a>
                <a href=artists_href class=artists_class>
                    <span inner_html=icon_mic()></span>
                    "Artists"
                </a>
                <a href=wanted_href class=wanted_class>
                    <span inner_html=icon_heart()></span>
                    "Wanted"
                </a>
            </nav>
            <div class="d7-sidebar-footer">
                <button type="button" class="d7-theme-btn" data-theme-toggle>
                    <span inner_html=icon_sun_moon()></span>
                    <span data-theme-label>"Dark"</span>
                </button>
            </div>
        </aside>
    }
}

// ── Helpers ─────────────────────────────────────────────────

fn status_label_text(status: &DownloadStatus, completed: usize, total: usize) -> String {
    match status {
        DownloadStatus::Queued => "Queued".to_string(),
        DownloadStatus::Resolving => "Resolving".to_string(),
        DownloadStatus::Downloading => {
            if total > 0 {
                format!("Downloading {}/{}", completed, total)
            } else {
                "Downloading".to_string()
            }
        }
        DownloadStatus::Completed => "Completed".to_string(),
        DownloadStatus::Failed => "Failed".to_string(),
    }
}

// ── Dashboard ───────────────────────────────────────────────

pub(crate) fn render_dashboard(
    monitored_count: usize,
    monitored_albums: usize,
    wanted_albums: usize,
    acquired_albums: usize,
    queued_jobs: usize,
    artists: &[MonitoredArtist],
    _albums: &[MonitoredAlbum],
    jobs: &[DownloadJob],
) -> String {
    let prefix = String::new();

    let artist_names: HashMap<i64, String> =
        artists.iter().map(|a| (a.id, a.name.clone())).collect();

    let has_completed = jobs
        .iter()
        .any(|j| matches!(j.status, DownloadStatus::Completed));

    let recent_jobs: Vec<_> = {
        let mut sorted = jobs.to_vec();
        sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sorted.into_iter().take(25).collect()
    };

    let body = view! {
        <div class="d7-wrapper">
            <Sidebar active="dashboard" prefix=prefix.clone() />
            <div class="d7-content">
                <div class="d7-topbar">
                    <h1 class="d7-topbar-title">"Dashboard"</h1>
                </div>
                <div class="d7-main">
                    <div class="d7-warning">{QUALITY_WARNING}</div>

                    <div class="d7-stats">
                        <div class="d7-stat-card">
                            <p class="d7-stat-label">"Artists"</p>
                            <p class="d7-stat-value">{monitored_count}</p>
                        </div>
                        <div class="d7-stat-card">
                            <p class="d7-stat-label">"Monitored Albums"</p>
                            <p class="d7-stat-value" data-dashboard-monitored>{monitored_albums}</p>
                        </div>
                        <div class="d7-stat-card">
                            <p class="d7-stat-label">"Wanted"</p>
                            <p class="d7-stat-value" data-dashboard-wanted>{wanted_albums}</p>
                        </div>
                        <div class="d7-stat-card">
                            <p class="d7-stat-label">"Acquired"</p>
                            <p class="d7-stat-value" data-dashboard-acquired>{acquired_albums}</p>
                        </div>
                        <div class="d7-stat-card">
                            <p class="d7-stat-label">"Active Jobs"</p>
                            <p class="d7-stat-value" data-dashboard-active-jobs>{queued_jobs}</p>
                        </div>
                    </div>

                    // Recent activity panel
                    <div class="d7-glass">
                        <div class="d7-glass-header">
                            <h2 class="d7-glass-title">"Recent Activity"</h2>
                            <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">
                                <form action="/library/scan-import" method="post" style="display:inline">
                                    <input type="hidden" name="return_to" value="/" />
                                    <button type="submit" class="d7-btn d7-btn-sm d7-btn-primary">"Scan Drive + Import"</button>
                                </form>
                                <form action="/library/retag" method="post" style="display:inline">
                                    <input type="hidden" name="return_to" value="/" />
                                    <button type="submit" class="d7-btn d7-btn-sm">"Retag Existing Files"</button>
                                </form>
                                {if has_completed {
                                    view! {
                                        <form action="/downloads/clear" method="post" style="display:inline">
                                            <input type="hidden" name="return_to" value="/" />
                                            <button type="submit" class="d7-btn d7-btn-sm">"Clear Completed"</button>
                                        </form>
                                    }.into_any()
                                } else {
                                    view! { <span></span> }.into_any()
                                }}
                            </div>
                        </div>
                        {if recent_jobs.is_empty() {
                            view! { <div class="d7-empty">"No download jobs yet."</div> }.into_any()
                        } else {
                            view! {
                                <table class="d7-table">
                                    <thead>
                                        <tr>
                                            <th>"Album"</th>
                                            <th>"Artist"</th>
                                            <th>"Quality"</th>
                                            <th>"Progress"</th>
                                            <th>"Status"</th>
                                            <th>"Updated"</th>
                                            <th>"Actions"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {recent_jobs.into_iter().map(|job| {
                                            let sc = status_class(&job.status);
                                            let st_label = status_label_text(
                                                &job.status,
                                                job.completed_tracks,
                                                job.total_tracks,
                                            );
                                            let progress = format!(
                                                "{}/{}",
                                                job.completed_tracks, job.total_tracks
                                            );
                                            let artist_name = artist_names
                                                .get(&job.artist_id)
                                                .cloned()
                                                .unwrap_or_else(|| format!("#{}", job.artist_id));
                                            let updated = job
                                                .updated_at
                                                .format("%Y-%m-%d %H:%M")
                                                .to_string();
                                            let is_queued = matches!(job.status, DownloadStatus::Queued);
                                            let is_failed = matches!(job.status, DownloadStatus::Failed);
                                            let job_id_str = job.id.to_string();
                                            let album_id_str = job.album_id.to_string();
                                            let error_msg = job.error.clone().unwrap_or_default();
                                            view! {
                                                <tr>
                                                    <td>
                                                        <div>{job.album_title}</div>
                                                        {if is_failed && !error_msg.is_empty() {
                                                            view! { <small class="d7-wanted-error">{error_msg}</small> }.into_any()
                                                        } else {
                                                            view! { <span></span> }.into_any()
                                                        }}
                                                    </td>
                                                    <td>{artist_name}</td>
                                                    <td>
                                                        <span class="pill d7-pill-muted">
                                                            {job.quality}
                                                        </span>
                                                    </td>
                                                    <td>{progress}</td>
                                                    <td><span class=sc>{st_label}</span></td>
                                                    <td class="d7-muted">{updated}</td>
                                                    <td>
                                                        {if is_queued {
                                                            view! {
                                                                <form action="/downloads/cancel" method="post" style="display:inline">
                                                                    <input type="hidden" name="job_id" value=job_id_str />
                                                                    <input type="hidden" name="return_to" value="/" />
                                                                    <button type="submit" class="d7-btn d7-btn-sm d7-btn-danger">"Cancel"</button>
                                                                </form>
                                                            }.into_any()
                                                        } else if is_failed {
                                                            view! {
                                                                <form action="/downloads/retry" method="post" style="display:inline">
                                                                    <input type="hidden" name="album_id" value=album_id_str />
                                                                    <input type="hidden" name="return_to" value="/" />
                                                                    <button type="submit" class="d7-btn d7-btn-sm">"Retry"</button>
                                                                </form>
                                                            }.into_any()
                                                        } else {
                                                            view! { <span class="d7-muted">{"\u{2014}"}</span> }.into_any()
                                                        }}
                                                    </td>
                                                </tr>
                                            }
                                        }).collect_view()}
                                    </tbody>
                                </table>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
    .to_html();

    layout("Dashboard", &body, &prefix)
}

// ── Artists ──────────────────────────────────────────────────

pub(crate) fn render_artists(
    query: String,
    results: Vec<HifiArtist>,
    monitored: Vec<MonitoredArtist>,
    albums: Vec<MonitoredAlbum>,
    error: Option<String>,
    prefix: &str,
) -> String {
    let prefix_owned = prefix.to_string();
    let artists_action = format!("{}/artists", prefix);
    let add_action = format!("{}/artists/add", prefix);
    let return_to_artists = format!("{}/artists", prefix);

    let monitored_count = monitored.len();
    let albums_by_artist = build_albums_by_artist(albums);

    let body = view! {
        <div class="d7-wrapper">
            <Sidebar active="artists" prefix=prefix_owned.clone() />
            <div class="d7-content">
                <div class="d7-topbar">
                    <h1 class="d7-topbar-title">"Artists"</h1>
                    <span class="d7-muted" style="font-size:13px">
                        {format!("{} monitored", monitored_count)}
                    </span>
                </div>
                <div class="d7-main">
                    // Search panel
                    <div class="d7-glass" style="margin-bottom:20px;overflow:visible;position:relative;z-index:50">
                        <div class="d7-glass-body" style="overflow:visible">
                            <form
                                action=artists_action
                                method="get"
                                style="display:flex;gap:8px;align-items:center;flex-wrap:wrap"
                            >
                                <div class="d7-search-wrapper">
                                    <input
                                        type="text"
                                        name="q"
                                        value=query
                                        class="d7-search-input"
                                        style="max-width:100%"
                                        placeholder="Search artist name..."
                                        autocomplete="off"
                                        data-instant-search
                                    />
                                    <div class="d7-search-dropdown" data-search-dropdown></div>
                                </div>
                                <button type="submit" class="d7-btn d7-btn-primary">
                                    "Search"
                                </button>
                            </form>
                        </div>
                    </div>

                    // Error banner
                    {match error {
                        Some(msg) => view! {
                            <div class="d7-warning" style="border-color:rgba(239,68,68,0.3);background:rgba(239,68,68,0.08);color:#dc2626">
                                {msg}
                            </div>
                        }.into_any(),
                        None => view! { <span></span> }.into_any(),
                    }}

                    // Search results
                    {if !results.is_empty() {
                        let add_c = add_action.clone();
                        let ret_c = return_to_artists.clone();
                        view! {
                            <div class="d7-glass">
                                <div class="d7-glass-header">
                                    <h2 class="d7-glass-title">"Search Results"</h2>
                                </div>
                                <div>
                                    {results.into_iter().map(|artist| {
                                        let id = artist.id;
                                        let name = artist.name.clone();
                                        let picture = artist.picture.clone().unwrap_or_default();
                                        let tidal_url = artist.url.clone().unwrap_or_default();
                                        let image_url = artist_image_url(&artist, 160);
                                        let profile_url = artist_profile_url(&artist);
                                        let fallback_initial = artist.name.chars().next()
                                            .map(|c| c.to_uppercase().to_string())
                                            .unwrap_or_else(|| "?".to_string());
                                        let add_inner = add_c.clone();
                                        let ret_inner = ret_c.clone();
                                        let id_str = id.to_string();
                                        view! {
                                            <div class="d7-search-result">
                                                {match image_url {
                                                    Some(url) => view! {
                                                        <img class="d7-artist-avatar" src=url alt="" />
                                                    }.into_any(),
                                                    None => view! {
                                                        <div class="d7-artist-fallback">{fallback_initial}</div>
                                                    }.into_any(),
                                                }}
                                                <div class="d7-search-result-info">
                                                    <div class="d7-search-result-name">{artist.name}</div>
                                                    <a class="d7-link" href=profile_url target="_blank" rel="noreferrer" style="font-size:12px">"View on Tidal"</a>
                                                </div>
                                                <form action=add_inner method="post" style="display:inline">
                                                    <input type="hidden" name="id" value=id_str />
                                                    <input type="hidden" name="name" value=name />
                                                    <input type="hidden" name="picture" value=picture />
                                                    <input type="hidden" name="tidal_url" value=tidal_url />
                                                    <input type="hidden" name="return_to" value=ret_inner />
                                                    <button type="submit" class="d7-btn d7-btn-primary d7-btn-sm">"+ Add"</button>
                                                </form>
                                            </div>
                                        }
                                    }).collect_view()}
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! { <span></span> }.into_any()
                    }}

                    // Monitored artists (crate cards linking to detail pages)
                    <div class="d7-glass">
                        <div class="d7-glass-header">
                            <h2 class="d7-glass-title">"Your Collection"</h2>
                        </div>
                        {if monitored.is_empty() {
                            view! { <div class="d7-empty">"No monitored artists yet. Search and add one above."</div> }.into_any()
                        } else {
                            view! {
                                <div class="d7-glass-body" style="padding:16px">
                                    <div class="d7-artist-grid">
                                        {monitored.into_iter().map(|artist| {
                                            let artist_albums = albums_by_artist
                                                .get(&artist.id)
                                                .cloned()
                                                .unwrap_or_default();
                                            let album_count = artist_albums.len();
                                            let wanted = artist_albums.iter().filter(|a| a.wanted).count();
                                            let acquired = artist_albums.iter().filter(|a| a.acquired).count();
                                            let album_count_text = format!(
                                                "{} albums \u{00b7} {} acquired \u{00b7} {} wanted",
                                                album_count, acquired, wanted
                                            );
                                            let fallback_initial = artist.name.chars().next()
                                                .map(|c| c.to_uppercase().to_string())
                                                .unwrap_or_else(|| "?".to_string());
                                            let artist_img = monitored_artist_image_url(&artist, 160);
                                            let detail_href = format!("/artists/{}", artist.id);

                                            view! {
                                                <a href=detail_href class="d7-artist-card" style="text-decoration:none;cursor:pointer">
                                                    {match artist_img {
                                                        Some(url) => view! {
                                                            <img class="d7-artist-avatar" src=url alt="" />
                                                        }.into_any(),
                                                        None => view! {
                                                            <div class="d7-artist-fallback">{fallback_initial}</div>
                                                        }.into_any(),
                                                    }}
                                                    <div class="d7-artist-info">
                                                        <div class="d7-artist-name">{artist.name}</div>
                                                        <div class="d7-artist-meta">{album_count_text}</div>
                                                    </div>
                                                </a>
                                            }
                                        }).collect_view()}
                                    </div>
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
    .to_html();

    layout("Artists", &body, prefix)
}

// ── Album sleeve helper ─────────────────────────────────────

fn render_album_sleeve(
    album: MonitoredAlbum,
    latest_jobs: &HashMap<i64, DownloadJob>,
    monitor_action: &str,
    return_to: &str,
) -> impl IntoView {
    let album_id_str = album.id.to_string();
    let album_title = album.title.clone();
    let release_date = album
        .release_date
        .clone()
        .unwrap_or_else(|| "\u{2014}".to_string());
    let album_type = album_type_label(album.album_type.as_deref(), &album.title);
    let is_explicit = album.explicit;
    let is_monitored = album.monitored;
    let is_wanted = album.wanted;
    let is_acquired = album.acquired;

    let cover_url = album_cover_url(&album, 640);
    let profile_url = album_profile_url(&album);

    let latest_job = latest_jobs.get(&album.id).cloned();
    let job_status = latest_job.as_ref().map(|j| j.status.clone());
    let job_progress = latest_job
        .as_ref()
        .map(|j| (j.completed_tracks, j.total_tracks));

    let monitor_next = (!is_monitored).to_string();

    let wanted_pill_text = if is_wanted { "Wanted" } else { "Not Wanted" };

    let status_pill_class = match &job_status {
        Some(s) => status_class(s).to_string(),
        None => "pill".to_string(),
    };
    let status_pill_text = match &job_status {
        Some(s) => status_label_text(
            s,
            job_progress.map(|(c, _)| c).unwrap_or(0),
            job_progress.map(|(_, t)| t).unwrap_or(0),
        ),
        None => "\u{2014}".to_string(),
    };

    let fallback_initial = album_title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let monitor_action = monitor_action.to_string();
    let return_to = return_to.to_string();
    let album_id_str_attr = album_id_str.clone();
    let album_id_str_monitor = album_id_str.clone();

    let monitor_title = if is_monitored {
        "Unmonitor album"
    } else {
        "Monitor album"
    };
    let monitor_label = if is_monitored { "Unmonitor" } else { "Monitor" };

    view! {
        <div class="d7-sleeve" data-album-row data-album-id=album_id_str_attr>
            // Cover
            <div class="d7-sleeve-cover-wrap">
                {match cover_url {
                    Some(url) => view! {
                        <img class="d7-sleeve-cover" src=url alt="" loading="lazy" />
                    }.into_any(),
                    None => view! {
                        <div class="d7-sleeve-fallback">{fallback_initial}</div>
                    }.into_any(),
                }}

                // Badge stickers
                {if is_wanted && !is_acquired {
                    view! { <span class="d7-badge d7-badge-wanted">"Wanted"</span> }.into_any()
                } else if is_acquired {
                    view! { <span class="d7-badge d7-badge-acquired">"Acquired"</span> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
                {if is_explicit {
                    view! { <span class="d7-badge d7-badge-explicit">"E"</span> }.into_any()
                } else {
                    view! { <span></span> }.into_any()
                }}
            </div>

            // Info panel
            <div class="d7-sleeve-info">
                <div class="d7-sleeve-title">
                    {match profile_url {
                        Some(url) => view! {
                            <a href=url target="_blank" rel="noreferrer">{album_title.clone()}</a>
                        }.into_any(),
                        None => view! {
                            <span>{album_title.clone()}</span>
                        }.into_any(),
                    }}
                </div>
                <div class="d7-sleeve-sub">{format!("{} · {}", release_date, album_type)}</div>

                // Status dots + pill
                <div class="d7-sleeve-status">
                    <span class=status_pill_class data-job-status>{status_pill_text}</span>
                    <span class="d7-muted" style="font-size:10px" data-wanted-pill>{wanted_pill_text}</span>
                </div>

                // Actions
                <div class="d7-sleeve-actions">
                    <form action=monitor_action method="post" style="display:inline">
                        <input type="hidden" name="album_id" value=album_id_str_monitor />
                        <input type="hidden" name="monitored" value=monitor_next />
                        <input type="hidden" name="return_to" value=return_to />
                        <button type="submit" class="d7-btn d7-sleeve-action-btn" title=monitor_title>{monitor_label}</button>
                    </form>
                    <button type="button" class="d7-tracklist-btn" data-tracklist-toggle=album_id_str.clone() title="Show tracks">{"\u{266b}"}</button>
                </div>
            </div>
            // Expandable tracklist
            <div class="d7-tracklist" data-tracklist-panel=album_id_str>
                <div class="d7-tracklist-loading">"Loading tracks..."</div>
            </div>
        </div>
    }
}

// ── Wanted ──────────────────────────────────────────────────

pub(crate) fn render_wanted(
    wanted: Vec<MonitoredAlbum>,
    artists: Vec<MonitoredArtist>,
    jobs: Vec<DownloadJob>,
    prefix: &str,
) -> String {
    let prefix_owned = prefix.to_string();
    let artist_names = build_artist_names(&artists);
    let latest_jobs = build_latest_jobs(jobs);
    let retry_action = format!("{}/downloads/retry", prefix);
    let monitor_action = format!("{}/albums/monitor", prefix);
    let return_to_wanted = format!("{}/wanted", prefix);

    // Group wanted albums by artist
    let albums_by_artist = build_albums_by_artist(wanted);

    let mut artist_order: Vec<(i64, String)> = albums_by_artist
        .keys()
        .map(|&aid| {
            let name = artist_names
                .get(&aid)
                .cloned()
                .unwrap_or_else(|| format!("Unknown ({})", aid));
            (aid, name)
        })
        .collect();
    artist_order.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

    let total_wanted = albums_by_artist.values().map(|v| v.len()).sum::<usize>();
    let wanted_is_empty = total_wanted == 0;
    let total_wanted_str = format!("{} albums", total_wanted);

    let body = view! {
        <div class="d7-wrapper">
            <Sidebar active="wanted" prefix=prefix_owned.clone() />
            <div class="d7-content">
                <div class="d7-topbar">
                    <h1 class="d7-topbar-title">"Wanted"</h1>
                    <span class="d7-muted" style="font-size:13px">{total_wanted_str.clone()}</span>
                </div>
                <div class="d7-main">
                    <div class="d7-glass">
                        <div class="d7-glass-header">
                            <h2 class="d7-glass-title">"Missing Albums"</h2>
                            <span class="d7-muted" style="font-size:12px">{total_wanted_str}</span>
                        </div>

                        {if wanted_is_empty {
                            view! {
                                <>
                                <div class="d7-empty" data-wanted-empty>"All albums acquired. Nothing wanted."</div>
                                </>
                            }.into_any()
                        } else {
                            let retry_c = retry_action.clone();
                            let monitor_c = monitor_action.clone();
                            let ret_c = return_to_wanted.clone();
                            view! {
                                <>
                                <div class="d7-empty hidden" data-wanted-empty>"All albums acquired. Nothing wanted."</div>
                                <div data-wanted-list>
                                    {artist_order.into_iter().map(|(artist_id, artist_name)| {
                                        let group_albums = albums_by_artist
                                            .get(&artist_id)
                                            .cloned()
                                            .unwrap_or_default();
                                        let retry_inner = retry_c.clone();
                                        let monitor_inner = monitor_c.clone();
                                        let ret_inner = ret_c.clone();
                                        let jobs_c = latest_jobs.clone();
                                        view! {
                                            <div class="d7-group-header">{artist_name}</div>
                                            {group_albums.into_iter().map(move |album| {
                                                render_wanted_row(
                                                    album,
                                                    jobs_c.clone(),
                                                    retry_inner.clone(),
                                                    monitor_inner.clone(),
                                                    ret_inner.clone(),
                                                )
                                            }).collect_view()}
                                        }
                                    }).collect_view()}
                                </div>
                                </>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
    .to_html();

    layout("Wanted", &body, prefix)
}

// ── Wanted row helper ───────────────────────────────────────

fn render_wanted_row(
    album: MonitoredAlbum,
    latest_jobs: HashMap<i64, DownloadJob>,
    retry_action: String,
    monitor_action: String,
    return_to: String,
) -> impl IntoView {
    let album_id_str = album.id.to_string();
    let album_title = album.title.clone();
    let release_date = album
        .release_date
        .clone()
        .unwrap_or_else(|| "\u{2014}".to_string());
    let is_explicit = album.explicit;

    let cover_url = album_cover_url(&album, 160);
    let profile_url = album_profile_url(&album);
    let fallback_initial = album_title
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let latest_job = latest_jobs.get(&album.id).cloned();
    let job_status = latest_job.as_ref().map(|j| j.status.clone());
    let job_error = latest_job.as_ref().and_then(|j| j.error.clone());
    let is_failed = matches!(job_status, Some(DownloadStatus::Failed));

    let sc = match &job_status {
        Some(s) => status_class(s).to_string(),
        None => "pill".to_string(),
    };
    let status_text = match &job_status {
        Some(s) => s.as_str().to_string(),
        None => "not queued".to_string(),
    };

    let error_class = if is_failed {
        "d7-wanted-error"
    } else {
        "d7-wanted-error hidden"
    };
    let error_text = job_error.unwrap_or_else(|| "Download failed".to_string());
    let retry_class = if is_failed { "" } else { "hidden" };

    let explicit_label = if is_explicit { " [E]" } else { "" };
    let meta_text = format!("{}{}", release_date, explicit_label);

    let return_to2 = return_to.clone();
    let album_id_str_attr = album_id_str.clone();
    let album_id_str_retry = album_id_str.clone();
    let album_id_str_monitor = album_id_str.clone();

    view! {
        <div class="d7-wanted-card" data-wanted-row data-album-id=album_id_str_attr>
            {match cover_url {
                Some(url) => view! {
                    <img class="d7-wanted-thumb" src=url alt="" />
                }.into_any(),
                None => view! {
                    <div class="d7-wanted-thumb-fallback">{fallback_initial}</div>
                }.into_any(),
            }}
            <div class="d7-wanted-info">
                <div class="d7-wanted-title">
                    {match profile_url {
                        Some(url) => view! {
                            <a href=url target="_blank" rel="noreferrer">{album_title.clone()}</a>
                        }.into_any(),
                        None => view! {
                            <span>{album_title.clone()}</span>
                        }.into_any(),
                    }}
                </div>
                <div class="d7-wanted-meta">{meta_text}</div>
                <small class=error_class data-job-error>{error_text}</small>
            </div>

            // Status pill
            <span class=sc data-job-status>{status_text}</span>

            // Actions
            <div class="d7-wanted-actions">
                <form action=retry_action method="post" style="display:inline" class=retry_class data-retry-form>
                    <input type="hidden" name="album_id" value=album_id_str_retry />
                    <input type="hidden" name="return_to" value=return_to />
                    <button type="submit" class="d7-btn d7-btn-sm d7-btn-danger">"Retry"</button>
                </form>
                <form action=monitor_action method="post" style="display:inline">
                    <input type="hidden" name="album_id" value=album_id_str_monitor />
                    <input type="hidden" name="monitored" value="false" />
                    <input type="hidden" name="return_to" value=return_to2 />
                    <button type="submit" class="d7-icon-btn" title="Unmonitor">{"\u{00d7}"}</button>
                </form>
            </div>
        </div>
    }
}

// ── Error page ──────────────────────────────────────────────

pub(crate) fn render_error(title: &str, message: &str) -> String {
    let prefix = String::new();
    let title_owned = title.to_string();
    let message_owned = message.to_string();

    let body = view! {
        <div class="d7-wrapper">
            <Sidebar active="" prefix=prefix.clone() />
            <div class="d7-content">
                <div class="d7-topbar">
                    <h1 class="d7-topbar-title">{title_owned}</h1>
                </div>
                <div class="d7-main">
                    <div class="d7-glass">
                        <div class="d7-glass-body">
                            <div class="d7-empty">
                                <p style="margin-bottom:12px">{message_owned}</p>
                                <a href="/artists" class="d7-btn d7-btn-primary">"Back to Artists"</a>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    }
    .to_html();

    layout("Error", &body, "")
}

// ── Artist detail ───────────────────────────────────────────

pub(crate) fn render_artist_detail(
    artist: MonitoredArtist,
    albums: Vec<MonitoredAlbum>,
    jobs: Vec<DownloadJob>,
    prefix: &str,
) -> String {
    let prefix_owned = prefix.to_string();
    let return_to = format!("{}/artists/{}", prefix, artist.id);
    let monitor_action = format!("{}/albums/monitor", prefix);
    let sync_action = format!("{}/artists/sync", prefix);
    let remove_action = format!("{}/artists/remove", prefix);
    let bulk_action = format!("{}/albums/bulk-monitor", prefix);

    let artist_img = monitored_artist_image_url(&artist, 320);
    let artist_profile = monitored_artist_profile_url(&artist);
    let fallback_initial = artist
        .name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());

    let album_count = albums.len();
    let monitored_count = albums.iter().filter(|a| a.monitored).count();
    let acquired_count = albums.iter().filter(|a| a.acquired).count();
    let wanted_count = albums.iter().filter(|a| a.wanted).count();

    let mut sorted_albums = albums;
    sorted_albums.sort_by(|a, b| {
        album_type_rank(a.album_type.as_deref(), &a.title)
            .cmp(&album_type_rank(b.album_type.as_deref(), &b.title))
            .then_with(|| b.release_date.cmp(&a.release_date))
            .then_with(|| a.title.cmp(&b.title))
    });

    let latest_jobs = build_latest_jobs(jobs);
    let artist_id_str = artist.id.to_string();
    let artist_id_str2 = artist_id_str.clone();
    let artist_id_str3 = artist_id_str.clone();

    let body = view! {
        <div class="d7-wrapper">
            <Sidebar active="artists" prefix=prefix_owned.clone() />
            <div class="d7-content">
                <div class="d7-topbar">
                    <h1 class="d7-topbar-title">{artist.name.clone()}</h1>
                    <a href="/artists" class="d7-btn d7-btn-sm" style="text-decoration:none">{"\u{2190} All Artists"}</a>
                </div>
                <div class="d7-main">
                    // Artist header card
                    <div class="d7-glass" style="margin-bottom:20px">
                        <div class="d7-glass-body" style="display:flex;gap:20px;align-items:center;flex-wrap:wrap">
                            // Avatar
                            {match artist_img {
                                Some(url) => view! {
                                    <img class="d7-artist-avatar" style="width:80px;height:80px;font-size:32px" src=url alt="" />
                                }.into_any(),
                                None => view! {
                                    <div class="d7-artist-fallback" style="width:80px;height:80px;font-size:32px">{fallback_initial}</div>
                                }.into_any(),
                            }}
                            <div style="flex:1;min-width:0">
                                <div style="font-size:22px;font-weight:700;margin-bottom:4px">{artist.name.clone()}</div>
                                <div class="d7-muted" style="font-size:13px;margin-bottom:8px">
                                    {format!("{} albums \u{00b7} {} monitored \u{00b7} {} acquired \u{00b7} {} wanted", album_count, monitored_count, acquired_count, wanted_count)}
                                </div>
                                <div style="display:flex;gap:6px;flex-wrap:wrap">
                                    <a href=artist_profile target="_blank" rel="noreferrer" class="d7-btn d7-btn-sm">"View on Tidal"</a>
                                    <form action=sync_action method="post" style="display:inline">
                                        <input type="hidden" name="artist_id" value=artist_id_str />
                                        <input type="hidden" name="return_to" value=return_to.clone() />
                                        <button type="submit" class="d7-btn d7-btn-sm">"Sync Albums"</button>
                                    </form>
                                    <form action=bulk_action.clone() method="post" style="display:inline">
                                        <input type="hidden" name="artist_id" value=artist_id_str2 />
                                        <input type="hidden" name="monitored" value="true" />
                                        <input type="hidden" name="return_to" value=return_to.clone() />
                                        <button type="submit" class="d7-btn d7-btn-sm d7-btn-primary">"Monitor All"</button>
                                    </form>
                                    <form action=bulk_action method="post" style="display:inline">
                                        <input type="hidden" name="artist_id" value=artist_id_str3 />
                                        <input type="hidden" name="monitored" value="false" />
                                        <input type="hidden" name="return_to" value=return_to.clone() />
                                        <button type="submit" class="d7-btn d7-btn-sm">"Unmonitor All"</button>
                                    </form>
                                    <form action=remove_action method="post" style="display:inline" onsubmit="return confirm('Remove this artist and all their albums?')">
                                        <input type="hidden" name="artist_id" value=artist.id.to_string() />
                                        <input type="hidden" name="return_to" value="/artists" />
                                        <button type="submit" class="d7-btn d7-btn-sm d7-btn-danger">"Remove Artist"</button>
                                    </form>
                                </div>
                            </div>
                        </div>
                    </div>

                    // Albums grid
                    <div class="d7-glass">
                        <div class="d7-glass-header">
                            <h2 class="d7-glass-title">"Discography"</h2>
                            <span class="d7-muted" style="font-size:12px">{format!("{} albums", album_count)}</span>
                        </div>
                        {if sorted_albums.is_empty() {
                            view! { <div class="d7-empty">"No albums synced. Hit Sync Albums to fetch from Tidal."</div> }.into_any()
                        } else {
                            let monitor_c = monitor_action.clone();
                            let ret_c = return_to.clone();
                            view! {
                                <div class="d7-glass-body" style="padding:16px">
                                    <div class="d7-album-grid">
                                        {sorted_albums.into_iter().map(|album| {
                                            render_album_sleeve(
                                                album,
                                                &latest_jobs,
                                                &monitor_c,
                                                &ret_c,
                                            )
                                        }).collect_view()}
                                    </div>
                                </div>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>
        </div>
    }
    .to_html();

    layout(&artist.name, &body, prefix)
}

fn album_type_rank(album_type: Option<&str>, title: &str) -> u8 {
    match album_type_label(album_type, title) {
        "Album" => 0,
        "EP" => 1,
        "Single" => 2,
        _ => 3,
    }
}

fn album_type_label(album_type: Option<&str>, title: &str) -> &'static str {
    if let Some(kind) = album_type {
        let k = kind.to_ascii_lowercase();
        if k.contains("ep") {
            return "EP";
        }
        if k.contains("single") {
            return "Single";
        }
        if k.contains("album") {
            return "Album";
        }
    }

    let t = title.to_ascii_lowercase();
    if t.contains(" ep") || t.ends_with("ep") || t.contains("(ep") {
        return "EP";
    }
    if t.contains(" single") || t.ends_with("single") || t.contains("(single") {
        return "Single";
    }
    "Album"
}
