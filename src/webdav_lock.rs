//! WebDAV 内存锁系统实现，支持超时清理。

use dav_server::davpath::DavPath;
use dav_server::ls::{DavLock, DavLockSystem, LsFuture};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use uuid::Uuid;
use xmltree::Element;

/// 进程内的 WebDAV 锁系统（带超时清理）。
#[derive(Debug, Clone)]
pub struct WebDavLockSystem {
    inner: Arc<Mutex<LockState>>,
}

#[derive(Debug, Default)]
struct LockState {
    locks: Vec<DavLock>,
}

impl WebDavLockSystem {
    /// 创建新的 WebDAV 锁系统实例。
    pub fn new() -> Box<Self> {
        Box::new(Self {
            inner: Arc::new(Mutex::new(LockState::default())),
        })
    }
}

impl DavLockSystem for WebDavLockSystem {
    fn lock(
        &self,
        path: &DavPath,
        principal: Option<&str>,
        owner: Option<&Element>,
        timeout: Option<Duration>,
        shared: bool,
        deep: bool,
    ) -> LsFuture<'_, Result<DavLock, DavLock>> {
        let inner = self.inner.clone();
        let path = path.clone();
        let owner = owner.cloned();
        let principal = principal.map(|value| value.to_string());
        Box::pin(async move {
            let mut state = inner.lock().await;
            state.prune_expired();
            state.check_ancestors(&path, principal.as_deref(), true, &[], shared)?;
            if deep {
                state.check_descendants(&path, principal.as_deref(), true, &[], shared)?;
            }

            let timeout_at = timeout.map(|d| SystemTime::now() + d);
            let lock = DavLock {
                token: Uuid::new_v4().urn().to_string(),
                path,
                principal: principal.clone(),
                owner,
                timeout_at,
                timeout,
                shared,
                deep,
            };
            state.locks.push(lock.clone());
            Ok(lock)
        })
    }

    fn unlock(&self, path: &DavPath, token: &str) -> LsFuture<'_, Result<(), ()>> {
        let inner = self.inner.clone();
        let path = path.clone();
        let token = token.to_string();
        Box::pin(async move {
            let mut state = inner.lock().await;
            state.prune_expired();
            let key = normalize_path(&path);
            let before = state.locks.len();
            state
                .locks
                .retain(|lock| !(normalize_path(&lock.path) == key && lock.token == token));
            if state.locks.len() == before {
                return Err(());
            }
            Ok(())
        })
    }

    fn refresh(
        &self,
        path: &DavPath,
        token: &str,
        timeout: Option<Duration>,
    ) -> LsFuture<'_, Result<DavLock, ()>> {
        let inner = self.inner.clone();
        let path = path.clone();
        let token = token.to_string();
        Box::pin(async move {
            let mut state = inner.lock().await;
            state.prune_expired();
            let key = normalize_path(&path);
            for lock in &mut state.locks {
                if normalize_path(&lock.path) == key && lock.token == token {
                    lock.timeout = timeout;
                    lock.timeout_at = timeout.map(|d| SystemTime::now() + d);
                    return Ok(lock.clone());
                }
            }
            Err(())
        })
    }

    fn check(
        &self,
        path: &DavPath,
        principal: Option<&str>,
        ignore_principal: bool,
        deep: bool,
        submitted_tokens: Vec<&str>,
    ) -> LsFuture<'_, Result<(), DavLock>> {
        let inner = self.inner.clone();
        let path = path.clone();
        let principal = principal.map(|value| value.to_string());
        let submitted_tokens: Vec<String> = submitted_tokens
            .into_iter()
            .map(|value| value.to_string())
            .collect();
        Box::pin(async move {
            let mut state = inner.lock().await;
            state.prune_expired();
            let token_refs: Vec<&str> = submitted_tokens.iter().map(String::as_str).collect();
            let principal_ref = principal.as_deref();
            state.check_ancestors(&path, principal_ref, ignore_principal, &token_refs, false)?;
            if deep {
                state.check_descendants(
                    &path,
                    principal_ref,
                    ignore_principal,
                    &token_refs,
                    false,
                )?;
            }
            Ok(())
        })
    }

    fn discover(&self, path: &DavPath) -> LsFuture<'_, Vec<DavLock>> {
        let inner = self.inner.clone();
        let path = path.clone();
        Box::pin(async move {
            let mut state = inner.lock().await;
            state.prune_expired();
            let key = normalize_path(&path);
            state
                .locks
                .iter()
                .filter(|lock| lock_applies_to_path(lock, &key))
                .cloned()
                .collect()
        })
    }

    fn delete(&self, path: &DavPath) -> LsFuture<'_, Result<(), ()>> {
        let inner = self.inner.clone();
        let path = path.clone();
        Box::pin(async move {
            let mut state = inner.lock().await;
            state.prune_expired();
            let key = normalize_path(&path);
            state
                .locks
                .retain(|lock| !is_descendant_or_same(&key, &normalize_path(&lock.path)));
            Ok(())
        })
    }
}

impl LockState {
    fn prune_expired(&mut self) {
        let now = SystemTime::now();
        self.locks.retain(|lock| match lock.timeout_at {
            Some(timeout_at) => timeout_at > now,
            None => true,
        });
    }

    #[allow(clippy::result_large_err)]
    fn check_ancestors(
        &self,
        path: &DavPath,
        principal: Option<&str>,
        ignore_principal: bool,
        submitted_tokens: &[&str],
        shared_ok: bool,
    ) -> Result<(), DavLock> {
        let key = normalize_path(path);
        for lock in &self.locks {
            if !lock_applies_to_path(lock, &key) {
                continue;
            }
            if holds_lock(lock, principal, ignore_principal, submitted_tokens) {
                continue;
            }
            if lock.shared && shared_ok {
                continue;
            }
            return Err(lock.clone());
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    fn check_descendants(
        &self,
        path: &DavPath,
        principal: Option<&str>,
        ignore_principal: bool,
        submitted_tokens: &[&str],
        shared_ok: bool,
    ) -> Result<(), DavLock> {
        let key = normalize_path(path);
        for lock in &self.locks {
            let lock_key = normalize_path(&lock.path);
            if !is_descendant(&key, &lock_key) {
                continue;
            }
            if holds_lock(lock, principal, ignore_principal, submitted_tokens) {
                continue;
            }
            if lock.shared && shared_ok {
                continue;
            }
            return Err(lock.clone());
        }
        Ok(())
    }
}

fn normalize_path(path: &DavPath) -> String {
    let mut value = path.as_url_string();
    if value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    value
}

fn is_descendant_or_same(ancestor: &str, path: &str) -> bool {
    if ancestor == "/" {
        return true;
    }
    if ancestor == path {
        return true;
    }
    path.starts_with(ancestor) && path.as_bytes().get(ancestor.len()) == Some(&b'/')
}

fn is_descendant(ancestor: &str, path: &str) -> bool {
    if ancestor == path {
        return false;
    }
    is_descendant_or_same(ancestor, path)
}

fn lock_applies_to_path(lock: &DavLock, path: &str) -> bool {
    let lock_path = normalize_path(&lock.path);
    if lock_path == path {
        return true;
    }
    lock.deep && is_descendant(&lock_path, path)
}

fn holds_lock(
    lock: &DavLock,
    principal: Option<&str>,
    ignore_principal: bool,
    submitted_tokens: &[&str],
) -> bool {
    if !submitted_tokens.iter().any(|token| *token == lock.token) {
        return false;
    }
    ignore_principal || principal == lock.principal.as_deref()
}
