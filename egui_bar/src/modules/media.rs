use std::time::{Duration, Instant};

use crate::animation::AnimationState;
use crate::state::AppState;
use crate::theme::colors;

use super::BarModule;

#[derive(Debug, Clone, Default)]
struct MediaInfo {
    player_name: String,
    title: String,
    artist: String,
    is_playing: bool,
    available: bool,
}

pub struct MediaModule {
    info: MediaInfo,
    last_poll: Instant,
    poll_interval: Duration,
}

impl MediaModule {
    pub fn new() -> Self {
        let mut module = Self {
            info: MediaInfo::default(),
            last_poll: Instant::now() - Duration::from_secs(10),
            poll_interval: Duration::from_secs(2),
        };
        module.poll();
        module
    }

    fn poll(&mut self) {
        self.info = Self::query_mpris().unwrap_or_default();
        self.last_poll = Instant::now();
    }

    fn query_mpris() -> Option<MediaInfo> {
        let finder = mpris::PlayerFinder::new().ok()?;

        // Try to find an active (playing) player first, fall back to any player
        let player = finder
            .find_active()
            .ok()
            .or_else(|| finder.find_all().ok().and_then(|mut v| v.pop()))?;

        let player_name = player.identity().to_string();

        let playback_status = player.get_playback_status().ok()?;
        let is_playing = playback_status == mpris::PlaybackStatus::Playing;

        let metadata = player.get_metadata().ok()?;
        let title = metadata
            .title()
            .unwrap_or("")
            .to_string();
        let artist = metadata
            .artists()
            .and_then(|a| a.first().map(|s| s.to_string()))
            .unwrap_or_default();

        Some(MediaInfo {
            player_name,
            title,
            artist,
            is_playing,
            available: true,
        })
    }

    fn toggle_playback() {
        if let Ok(finder) = mpris::PlayerFinder::new() {
            if let Ok(player) = finder.find_active() {
                let _ = player.play_pause();
            }
        }
    }

    fn next_track() {
        if let Ok(finder) = mpris::PlayerFinder::new() {
            if let Ok(player) = finder.find_active() {
                let _ = player.next();
            }
        }
    }

    fn prev_track() {
        if let Ok(finder) = mpris::PlayerFinder::new() {
            if let Ok(player) = finder.find_active() {
                let _ = player.previous();
            }
        }
    }
}

impl BarModule for MediaModule {
    fn id(&self) -> &str {
        "media"
    }

    fn name(&self) -> &str {
        "Media"
    }

    fn update(&mut self, _state: &AppState) -> bool {
        if self.last_poll.elapsed() >= self.poll_interval {
            self.poll();
            true
        } else {
            false
        }
    }

    fn render_bar(&mut self, ui: &mut egui::Ui, _state: &mut AppState, _anim: &mut AnimationState) {
        if !self.info.available {
            return; // No player, hide module
        }

        let status_icon = if self.info.is_playing { "▶" } else { "⏸" };

        // Truncate title to keep bar compact
        let display_title = if self.info.title.len() > 20 {
            format!("{}…", &self.info.title[..19])
        } else {
            self.info.title.clone()
        };

        let label = if self.info.artist.is_empty() {
            format!("{} {}", status_icon, display_title)
        } else {
            let display_artist = if self.info.artist.len() > 15 {
                format!("{}…", &self.info.artist[..14])
            } else {
                self.info.artist.clone()
            };
            format!("{} {} - {}", status_icon, display_artist, display_title)
        };

        let color = if self.info.is_playing {
            colors::SUCCESS
        } else {
            colors::TEXT_SUBTLE
        };

        let resp = ui.add(egui::Button::new(egui::RichText::new(&label).color(color)));

        if resp.clicked() {
            Self::toggle_playback();
            // Immediate feedback
            self.info.is_playing = !self.info.is_playing;
        }

        let hovered = resp.hovered();
        resp.on_hover_text(format!(
            "{}\n{}{}\nClick: play/pause | Scroll: prev/next",
            self.info.player_name,
            if self.info.artist.is_empty() {
                String::new()
            } else {
                format!("{} - ", self.info.artist)
            },
            self.info.title,
        ));

        // Scroll for prev/next
        if hovered {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll > 1.0 {
                Self::next_track();
            } else if scroll < -1.0 {
                Self::prev_track();
            }
        }
    }
}
