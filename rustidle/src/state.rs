use wllib::dispatch::EventHandler;
use wllib::protocols::ext_idle_notification_v1;
use wllib::registry::{GlobalHandler, bind, clamp_version};
use wllib::transport::Connection;

use crate::config::{Config, Entry, MAX_ENTRIES};

// tracks registered object ids
#[derive(Default)]
pub struct Global {
  // wl_seat global id
  pub seat_id: u32,
  // ext_idle_notifier global id
  pub idle_notifier_id: u32,
}

#[derive(Default)]
pub struct Notification {
  pub id: u32,
  pub entry: Entry,
}

pub struct State {
  pub global: Global,
  pub config: Config,
  pub notifications: [Notification; MAX_ENTRIES],
}
impl State {
  #[must_use]
  pub fn init(config: Config) -> Self {
    let mut notifications = core::array::from_fn(|_| Notification::default());
    (0..config.entry_len).for_each(|i| notifications[i].entry = config.entries[i]);

    Self {
      global: Global::default(),
      config,
      notifications,
    }
  }
}
impl GlobalHandler for State {
  // bind globals matching interfaces we want. we bind to the minimum of the client's wanted
  // version and the server's advertised version
  fn on_global(&mut self, conn: &mut Connection, name: u32, interface: &str, version: u32) {
    match interface {
      "wl_seat" => {
        if self.global.seat_id != 0 {
          return;
        }
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(1, version), id) {
          Ok(()) => self.global.seat_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      "ext_idle_notifier_v1" => {
        let id = conn.alloc_id();
        match bind(conn, name, interface, clamp_version(2, version), id) {
          Ok(()) => self.global.idle_notifier_id = id,
          Err(e) => e.write_diagnostic(),
        }
      }
      _ => (),
    }
  }
}
impl EventHandler for State {
  fn handle_event(&mut self, _conn: &mut Connection, sender: u32, opcode: u16, _data: &[u8]) {
    for notif in &self.notifications {
      if notif.id != sender {
        continue;
      }
      match opcode {
        ext_idle_notification_v1::event::IDLED => {
          if let Some(argv) = notif.entry.idle_argv {
            self.config.spawn(argv);
          }
          return;
        }
        ext_idle_notification_v1::event::RESUMED => {
          if let Some(argv) = notif.entry.resume_argv {
            self.config.spawn(argv);
          }
          return;
        }
        _ => (),
      }
    }
  }
}
