//! Manages image playback, decoder slots and frame deadlines.

use super::*;

/// Animation: which entries play, their players on the worker, and the swap of frames.
impl ImageStore {
    /// Turns animation on or off (off until a shell asks). Turning it off drops every player and
    /// puts local images back on their first frame (they load again when next drawn); a remote
    /// one keeps the frame it shows. Turning it on lets what is already loaded start when it is
    /// next on screen. True when the setting changed.
    pub(in super::super) fn set_animation(&mut self, on: bool) -> bool {
        if self.play.enabled == on {
            return false;
        }
        self.play.enabled = on;
        for id in 0..self.entries.len() {
            let entry = &mut self.entries[id];
            if entry.plays == 0 || !matches!(entry.slot, Slot::Ready(_)) {
                continue;
            }
            if on {
                if entry.anim.is_none() {
                    entry.anim = Some(Anim::new(0));
                    self.play.count += 1;
                }
                continue;
            }
            let Some(anim) = entry.anim.take() else {
                continue;
            };
            if matches!(entry.key, ImageKey::File { .. })
                && let Slot::Ready(image) = std::mem::replace(&mut entry.slot, Slot::Evicted)
            {
                self.bytes -= image.rgba.len() as u64;
            }
            self.release(id as u32, anim);
        }
        true
    }

    /// Animations playing from kept frames, without a decoder.
    pub(super) fn looping(&self) -> usize {
        self.play
            .kept_ids
            .iter()
            .filter_map(|&id| self.entries[id as usize].anim.as_ref())
            .filter(|anim| anim.looping)
            .count()
    }

    /// What the worker calls after each frame it has ready. Set before the first image loads.
    pub(in super::super) fn set_waker(&mut self, waker: Waker) {
        self.play.waker = Some(waker);
    }

    #[cfg(test)]
    pub(in super::super) fn set_animation_budget(&mut self, bytes: u64) {
        self.play.budget = bytes;
    }

    /// Players on the worker now.
    #[cfg(test)]
    pub(in super::super) fn decoders(&self) -> usize {
        self.play.tokens.len()
    }

    /// Whether any entry plays or waits to, so a caller with nothing animated does no work.
    pub(in super::super) fn animating(&self) -> bool {
        self.play.enabled && self.play.count > 0
    }

    /// Whether entry `id` plays or waits to.
    pub(in super::super) fn animated(&self, id: u32) -> bool {
        self.entries
            .get(id as usize)
            .is_some_and(|e| e.anim.is_some())
    }

    pub(super) fn send(&mut self, job: Job) -> bool {
        if self.play.worker.is_none() {
            self.play.worker = Worker::spawn(self.play.waker.clone());
        }
        match &self.play.worker {
            Some(worker) => worker.jobs.send(job).is_ok(),
            None => false,
        }
    }

    /// Gives back what an `Anim` taken off entry `id` held.
    pub(super) fn release(&mut self, id: u32, anim: Anim) {
        self.play.count -= 1;
        if anim.token != 0 {
            self.play.tokens.remove(&anim.token);
            self.send(Job::Drop(anim.token));
        }
        if !anim.kept.is_empty() {
            self.play.kept_ids.retain(|&other| other != id);
        }
        self.recount();
    }

    /// The bytes live players and kept frames hold, from their entries.
    pub(super) fn recount(&mut self) {
        let players: u64 = self
            .play
            .tokens
            .values()
            .map(|&id| self.entries[id as usize].plays)
            .sum();
        let kept: u64 = self
            .play
            .kept_ids
            .iter()
            .filter_map(|&id| self.entries[id as usize].anim.as_ref())
            .map(Anim::kept_bytes)
            .sum();
        self.play.bytes = players + kept;
    }

    /// Offers the bytes entry `id` was just decoded from: if they may animate and the canvas is
    /// within the limit, the entry gets a player (now, when there is room and animation is on).
    pub(in super::super) fn offer(&mut self, id: u32, bytes: Cow<'_, [u8]>) {
        let Some(entry) = self.entries.get_mut(id as usize) else {
            return;
        };
        let Slot::Ready(image) = &entry.slot else {
            return;
        };
        let pixels = u64::from(image.width) * u64::from(image.height);
        if pixels > MAX_ANIMATION_PIXELS || !anim::candidate(&bytes) {
            return;
        }
        // The file, the canvas, its saved copy and the frame in flight (4 bytes a pixel each).
        entry.plays = bytes.len() as u64 + pixels * 12;
        if !self.play.enabled || entry.anim.is_some() {
            return;
        }
        entry.anim = Some(Anim::new(self.play.tick));
        self.play.count += 1;
        if self.make_room(id) {
            self.start(id, bytes.into_owned());
        }
    }

    /// Starts a player for an entry that has an `Anim` without one.
    pub(super) fn start(&mut self, id: u32, bytes: Vec<u8>) {
        self.play.last_token += 1;
        let token = self.play.last_token;
        let size = self.entries[id as usize].anim.as_ref().and_then(|a| a.size);
        if !self.send(Job::Start { token, bytes, size }) {
            // No worker thread: the image stays still.
            self.still(id);
            return;
        }
        if let Some(anim) = self.entries[id as usize].anim.as_mut() {
            anim.token = token;
            anim.next = None;
            anim.shown = None;
        }
        self.play.tokens.insert(token, id);
        self.recount();
    }

    /// Makes room in the budget for entry `id`'s player by taking players and kept frames from
    /// animations that were not on screen at the last look, longest unseen first. False when it
    /// still does not fit.
    pub(super) fn make_room(&mut self, id: u32) -> bool {
        let cost = self.entries[id as usize].plays;
        if cost > self.play.budget {
            return false;
        }
        while self.play.bytes + cost > self.play.budget {
            let tick = self.play.tick;
            let oldest = self
                .play
                .tokens
                .values()
                .chain(&self.play.kept_ids)
                .filter_map(|&other| {
                    let seen = self.entries[other as usize].anim.as_ref()?.seen;
                    (seen < tick && other != id).then_some((seen, other))
                })
                .min();
            let Some((_, other)) = oldest else {
                return false;
            };
            self.park(other);
        }
        true
    }

    /// Takes an entry's player and kept frames away; the entry keeps the frame it shows and
    /// starts over when next shown.
    pub(super) fn park(&mut self, id: u32) {
        let Some(anim) = self.entries[id as usize].anim.as_mut() else {
            return;
        };
        let token = std::mem::take(&mut anim.token);
        anim.next = None;
        anim.shown = None;
        anim.kept.clear();
        anim.looping = false;
        self.play.tokens.remove(&token);
        self.play.kept_ids.retain(|&other| other != id);
        if token != 0 {
            self.send(Job::Drop(token));
        }
        self.recount();
    }

    /// Gives a shown entry without a player one again: a file is read again, a remote image goes
    /// back to the shell's queue (its arrival offers the bytes).
    pub(super) fn revive(&mut self, id: u32) {
        if !self.make_room(id) {
            return;
        }
        let key = self.entries[id as usize].key.clone();
        match key {
            ImageKey::File { path, .. } => match image::read(&path) {
                Some(bytes) if anim::candidate(&bytes) => self.start(id, bytes),
                _ => self.still(id),
            },
            ImageKey::Remote(url) => {
                self.still(id);
                if !self.remote_new.contains(&url) {
                    self.remote_new.push(url);
                }
            }
            _ => self.still(id),
        }
    }

    /// Drops an entry's `Anim`. The frame it shows stays: one drawn from a scaled copy has no
    /// full-size pixels of its own, so the entry falls back to the last full frame it held.
    pub(super) fn still(&mut self, id: u32) {
        if let Some(anim) = self.entries[id as usize].anim.take() {
            self.release(id, anim);
        }
    }

    /// The pixels to draw entry `id` with into a box of `w` by `h` device px. For an animation
    /// shown below half size this is the current frame already scaled to that box (the worker
    /// scales, once per frame, so a tick costs a copy and not the area filter); the box is
    /// remembered so the next frames come at its size. Everything else is [`pixels`](Self::pixels).
    /// True with pixels that are opaque and exactly the size of the box.
    pub(in super::super) fn pixels_at(
        &mut self,
        id: u32,
        w: f32,
        h: f32,
    ) -> Option<(Rc<DecodedImage>, bool)> {
        let entry = self.entries.get_mut(id as usize)?;
        if let (Some(anim), Slot::Ready(full)) = (entry.anim.as_mut(), &entry.slot) {
            let (dw, dh) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
            let small = dw * 2 < full.width || dh * 2 < full.height;
            let size = small.then_some((dw, dh));
            if anim.size != size {
                // Another zoom or width: frames kept for the old box are of no use.
                anim.size = size;
                let had = !anim.kept.is_empty();
                anim.kept.clear();
                anim.looping = false;
                if had {
                    self.play.kept_ids.retain(|&other| other != id);
                    self.recount();
                }
            }
            let anim = self.entries[id as usize].anim.as_ref()?;
            if let Some(drawn) = &anim.drawn
                && Some((drawn.width, drawn.height)) == size
            {
                return Some((drawn.clone(), true));
            }
        }
        self.pixels(id).map(|image| (image, false))
    }

    /// Takes what the worker has ready, then puts up every frame that is due among the entries
    /// in `visible` (the animated images on screen; only they move). Returns the entries whose
    /// pixels changed and the time the next frame in hand is due; a frame still being decoded has
    /// no time, the worker's wake announces it.
    pub(in super::super) fn advance(
        &mut self,
        now: Instant,
        visible: &[u32],
    ) -> (Vec<u32>, Option<Instant>) {
        let replies: Vec<Reply> = match &self.play.worker {
            Some(worker) => worker.replies.try_iter().collect(),
            None => Vec::new(),
        };
        for reply in replies {
            match reply {
                Reply::Frame(token, step) => {
                    let Some(&id) = self.play.tokens.get(&token) else {
                        continue;
                    };
                    if let Some(anim) = self.entries[id as usize].anim.as_mut() {
                        anim.next = Some(step);
                    }
                }
                Reply::End(token) => {
                    // Still after all, or its last loop is over: the frame shown stays.
                    if let Some(&id) = self.play.tokens.get(&token) {
                        self.entries[id as usize].plays = 0;
                        self.finish(id);
                    }
                }
            }
        }
        self.play.tick += 1;
        let tick = self.play.tick;
        for &id in visible {
            if let Some(anim) = self
                .entries
                .get_mut(id as usize)
                .and_then(|e| e.anim.as_mut())
            {
                anim.seen = tick;
            }
        }
        // Off screen, the clock stands still.
        let playing: Vec<u32> = self
            .play
            .tokens
            .values()
            .chain(&self.play.kept_ids)
            .copied()
            .collect();
        for id in playing {
            if let Some(anim) = self.entries[id as usize].anim.as_mut()
                && anim.seen != tick
            {
                anim.shown = None;
            }
        }
        let mut changed = Vec::new();
        let mut next: Option<Instant> = None;
        let mut soonest = |due: Instant| next = Some(next.map_or(due, |n: Instant| n.min(due)));
        for &id in visible {
            let Some(anim) = self
                .entries
                .get_mut(id as usize)
                .and_then(|e| e.anim.as_mut())
            else {
                continue;
            };
            if anim.looping {
                // Every frame of the loop is kept at the size shown: no decoder, only a clock.
                let shown = *anim.shown.get_or_insert(now);
                let count = anim.kept.len();
                let after = anim.kept[(anim.cursor + 1) % count].1;
                let due = shown + after;
                if now < due {
                    soonest(due);
                    continue;
                }
                anim.cursor = (anim.cursor + 1) % count;
                anim.drawn = Some(anim.kept[anim.cursor].0.clone());
                let from = on_time(now, due, after);
                anim.shown = Some(from);
                soonest(from + anim.kept[(anim.cursor + 1) % count].1);
                self.play.frames += 1;
                changed.push(id);
                continue;
            }
            if anim.token == 0 {
                self.revive(id);
                continue;
            }
            let shown = *anim.shown.get_or_insert(now);
            let Some(after) = anim.next.as_ref().map(|step| step.after) else {
                continue;
            };
            let due = shown + after;
            if now < due {
                soonest(due);
                continue;
            }
            let Some(step) = anim.next.take() else {
                continue;
            };
            anim.shown = Some(on_time(now, due, after));
            self.show(id, step);
            self.play.frames += 1;
            changed.push(id);
        }
        (changed, next)
    }

    /// Puts a decoded frame up on entry `id` and asks for the one after it (or, once a whole
    /// loop is kept at the size shown, lets the decoder go).
    pub(super) fn show(&mut self, id: u32, step: anim::Step) {
        let entry = &mut self.entries[id as usize];
        let Some(anim) = entry.anim.as_mut() else {
            return;
        };
        let image = Rc::new(DecodedImage {
            width: step.width,
            height: step.height,
            baseline: step.height,
            rgba: step.rgba,
        });
        let frame_bytes = image.rgba.len() as u64;
        let had = !anim.kept.is_empty();
        let mut round = false;
        if !step.scaled {
            // A full-size frame is the entry's own pixels, as a still image's are.
            anim.drawn = None;
            anim.kept.clear();
            self.bytes += frame_bytes;
            if let Slot::Ready(old) = std::mem::replace(&mut entry.slot, Slot::Ready(image)) {
                self.bytes -= old.rgba.len() as u64;
            }
        } else {
            // A frame at the size shown is kept, from the first of a loop on, while the loop
            // fits; when the first comes round again the whole loop is in hand.
            let fits = anim.size == Some((step.width, step.height))
                && step.forever
                && anim.kept_bytes() + frame_bytes <= MAX_ANIMATION_KEPT_BYTES
                && self.play.bytes + frame_bytes <= self.play.budget;
            if !fits {
                anim.kept.clear();
            } else if step.index == 0 && !anim.kept.is_empty() {
                round = true;
            } else if step.index as usize == anim.kept.len() {
                anim.kept.push((image.clone(), step.after));
            } else {
                anim.kept.clear();
            }
            anim.drawn = Some(image);
        }
        let (token, size) = (anim.token, anim.size);
        let has = !anim.kept.is_empty();
        if round {
            anim.looping = true;
            anim.cursor = 0;
            anim.token = 0;
            anim.next = None;
            self.play.tokens.remove(&token);
            self.send(Job::Drop(token));
        } else {
            self.send(Job::Next(token, size));
        }
        if has && !had {
            self.play.kept_ids.push(id);
        } else if had && !has {
            self.play.kept_ids.retain(|&other| other != id);
        }
        self.recount();
    }

    /// Ends entry `id`'s animation on the frame it shows.
    pub(super) fn finish(&mut self, id: u32) {
        let entry = &mut self.entries[id as usize];
        let Some(mut anim) = entry.anim.take() else {
            return;
        };
        // A frame drawn from a scaled copy is not the entry's own pixels; the last frame the
        // player sends comes full size, so only an animation cut short can end on a scaled one,
        // and it then shows the last full frame it held.
        anim.drawn = None;
        self.release(id, anim);
    }
}

/// When a frame put up at `now` counts as shown: when it was due, so delays do not drift, unless
/// it is more than its own delay late (a slow decode, a hidden window), then now.
fn on_time(now: Instant, due: Instant, after: Duration) -> Instant {
    if now.duration_since(due) > after {
        now
    } else {
        due
    }
}
