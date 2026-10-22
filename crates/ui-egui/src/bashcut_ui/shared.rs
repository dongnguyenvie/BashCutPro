//! A long-lived service (plugin host, agent, batch) held once per app in egui context data, so
//! `FilmcraftApp` gains no field (`bashcut/review/ui.md` D3).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The service under `id`, created with `init` on first use.
pub fn get_or_init<T: Send + 'static>(ctx: &egui::Context, id: egui::Id, init: impl FnOnce() -> T) -> Arc<Mutex<T>> {
    if let Some(h) = existing(ctx, id) {
        return h;
    }
    // built outside `data_mut`: `init` may read context data itself
    let h = Arc::new(Mutex::new(init()));
    ctx.data_mut(|d| d.insert_temp(id, h.clone()));
    h
}

/// The service under `id` if something created it already.
pub fn existing<T: Send + 'static>(ctx: &egui::Context, id: egui::Id) -> Option<Arc<Mutex<T>>> {
    ctx.data(|d| d.get_temp::<Arc<Mutex<T>>>(id))
}

/// Lock it, also after a panic in another holder.
pub fn lock<T>(h: &Arc<Mutex<T>>) -> MutexGuard<'_, T> {
    h.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_once_then_found() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("bashcut.test.shared");
        assert!(existing::<u32>(&ctx, id).is_none());
        let a = get_or_init(&ctx, id, || 7u32);
        let b = get_or_init(&ctx, id, || 9u32);
        assert!(Arc::ptr_eq(&a, &b), "the second call finds the first");
        *lock(&a) += 1;
        assert_eq!(existing::<u32>(&ctx, id).map(|h| *lock(&h)), Some(8));
    }
}
