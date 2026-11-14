use std::io;
use std::time::{Duration, Instant};

use crate::{FrameTransport, Kitty};
#[cfg(unix)]
use crate::{fill_shm, open_shm};

const HANDOFF_STALE_AFTER: Duration = Duration::from_secs(3);
const HANDOFF_MAX_PENDING: usize = 4096;

pub(super) enum Payload {
    #[cfg(unix)]
    Shm(String),
    File(std::path::PathBuf),
}

impl Payload {
    fn remove(self) {
        match self {
            #[cfg(unix)]
            Payload::Shm(name) => {
                let _ = rustix::shm::unlink(&name);
            }
            Payload::File(path) => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Payloads {
    seq: u64,
    pending: std::collections::VecDeque<(Instant, Payload)>,
}

impl Payloads {
    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    fn track(&mut self, payload: Payload) {
        self.remove_stale();
        self.pending.push_back((Instant::now(), payload));
    }

    pub(super) fn remove_stale(&mut self) {
        while let Some((at, _)) = self.pending.front() {
            if at.elapsed() < HANDOFF_STALE_AFTER && self.pending.len() <= HANDOFF_MAX_PENDING {
                break;
            }
            self.remove_front();
        }
    }

    pub(super) fn remove_all(&mut self) {
        while !self.pending.is_empty() {
            self.remove_front();
        }
    }

    fn remove_front(&mut self) {
        if let Some((_, payload)) = self.pending.pop_front() {
            payload.remove();
        }
    }
}

impl Kitty {
    #[cfg(unix)]
    pub(super) fn shm_name(&self, seq: u64) -> String {
        format!("/px-{}-{}-{seq}", std::process::id(), self.terminal_id)
    }

    #[cfg(unix)]
    pub(super) fn shm_probe_name(&self) -> String {
        format!("/px-{}-{}-probe", std::process::id(), self.terminal_id)
    }

    fn temp_payload_path(&self, seq: u64) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tty-graphics-protocol-px-{}-{}-{seq}.rgba",
            std::process::id(),
            self.terminal_id
        ))
    }

    #[cfg(unix)]
    fn hand_off_shm_with(&mut self, len: usize, fill: impl FnOnce(&mut [u8])) -> io::Result<String> {
        let prefix = self.shm_name(0);
        let pool = self.shm_pool.get_or_insert_with(|| crate::shm_pool::ShmPool::new(prefix));
        if let Some(mut region) = pool.take(len) {
            fill(&mut region.bytes()[..len]);
            let name = region.name.clone();
            self.payloads.track(Payload::Shm(name.clone()));
            return Ok(name);
        }
        let seq = self.payloads.next_seq();
        let name = self.shm_name(seq);
        let fd = match open_shm(&name, len) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let _ = rustix::shm::unlink(&name);
                open_shm(&name, len)?
            }
            other => other?,
        };
        fill_shm(&fd, len, fill)?;
        self.payloads.track(Payload::Shm(name.clone()));
        Ok(name)
    }

    #[cfg(unix)]
    pub(crate) fn hand_off_shm(&mut self, data: &[u8]) -> io::Result<String> {
        self.hand_off_shm_with(data.len(), |out| out.copy_from_slice(data))
    }

    fn hand_off_temp_file(&mut self, data: &[u8]) -> io::Result<String> {
        let seq = self.payloads.next_seq();
        let path = self.temp_payload_path(seq);
        std::fs::write(&path, data)?;
        self.payloads.track(Payload::File(path.clone()));
        Ok(path.to_string_lossy().into_owned())
    }

    pub(crate) fn patch_medium(&self) -> Option<crate::kitty::Medium> {
        match self.transport {
            FrameTransport::Inline => None,
            #[cfg(unix)]
            FrameTransport::Host => None,
            #[cfg(unix)]
            FrameTransport::Shared => Some(crate::kitty::Medium::Shared),
            FrameTransport::File => Some(crate::kitty::Medium::Temporary),
        }
    }

    pub(crate) fn hand_off_payload(
        &mut self,
        medium: crate::kitty::Medium,
        len: usize,
        fill: impl FnOnce(&mut [u8]),
    ) -> io::Result<String> {
        match medium {
            #[cfg(unix)]
            crate::kitty::Medium::Shared => self.hand_off_shm_with(len, fill),
            #[cfg(windows)]
            crate::kitty::Medium::Shared => Err(io::Error::from(io::ErrorKind::Unsupported)),
            crate::kitty::Medium::File | crate::kitty::Medium::Temporary => {
                let mut data = vec![0u8; len];
                fill(&mut data);
                self.hand_off_temp_file(&data)
            }
        }
    }

    pub(crate) fn has_ready_payload(&self, medium: crate::kitty::Medium, len: usize) -> bool {
        #[cfg(unix)]
        return medium != crate::kitty::Medium::Shared
            || self.shm_pool.as_ref().is_some_and(|pool| pool.has_ready(len));
        #[cfg(windows)]
        {
            let _ = len;
            medium != crate::kitty::Medium::Shared
        }
    }

    pub(crate) fn hand_off_frame(&mut self, pixels: &[u8]) -> io::Result<Option<(crate::kitty::Medium, String)>> {
        Ok(match self.transport {
            FrameTransport::Inline => None,
            #[cfg(unix)]
            FrameTransport::Host => None,
            #[cfg(unix)]
            FrameTransport::Shared => Some((crate::kitty::Medium::Shared, self.hand_off_shm(pixels)?)),
            FrameTransport::File => Some((crate::kitty::Medium::File, self.write_frame_file(pixels)?)),
        })
    }

}
