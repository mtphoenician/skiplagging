pub mod budget;
pub mod cache;
pub mod catalog;
pub mod config;
pub mod db;
pub mod engines;
pub mod fx;
pub mod http;
pub mod metros;
pub mod models;
pub mod providers;

pub(crate) fn mutex_lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
