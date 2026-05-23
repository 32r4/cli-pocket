//! `IndexedDB`-backed [`KeyValueStore`].
//!
//! `IndexedDB` is event-driven on the JS side: every operation produces an
//! `IDBRequest` whose `onsuccess` / `onerror` callbacks fire later. We bridge
//! each request into a [`futures_channel::oneshot`] so callers can `.await`
//! the result.
//!
//! Layout: one database (`cli-pocket`) with a single object store (`kv`).
//! Keys are `&str`, values are `Vec<u8>` (stored as `Uint8Array`).
//!
//! Identity bytes and (eventually) resume tokens live here; nothing else in
//! `client-core` knows about filesystem paths.

use async_trait::async_trait;
use cli_pocket_client_core::{ClientError, ClientResult, KeyValueStore};
use futures_channel::oneshot;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    IdbDatabase, IdbFactory, IdbObjectStore, IdbOpenDbRequest, IdbRequest, IdbTransactionMode,
};

const DB_NAME: &str = "cli-pocket";
const DB_VERSION: u32 = 1;
const STORE: &str = "kv";

/// [`KeyValueStore`] backed by `IndexedDB`.
///
/// Constructed via [`IdbStore::open`]; the returned future resolves once the
/// database is open and the `kv` object store exists. Closures registered
/// during `open` are intentionally `.forget()`-ed: `IndexedDB` only invokes
/// each one once, after which the database itself owns the live JS-side
/// references it needs.
#[allow(dead_code)] // Wired into the public JS API by Task F13.
pub struct IdbStore {
    db: IdbDatabase,
}

impl IdbStore {
    /// Open (or create) the `cli-pocket` database and ensure the `kv` object
    /// store exists. Resolves when `onsuccess` fires.
    #[allow(dead_code)] // Wired into the public JS API by Task F13.
    pub async fn open() -> ClientResult<Self> {
        let factory: IdbFactory = web_sys::window()
            .ok_or_else(|| ClientError::Internal("no window".into()))?
            .indexed_db()
            .map_err(|e| ClientError::Internal(format!("indexed_db: {e:?}")))?
            .ok_or_else(|| ClientError::Internal("no indexedDB".into()))?;
        let req: IdbOpenDbRequest = factory
            .open_with_u32(DB_NAME, DB_VERSION)
            .map_err(|e| ClientError::Internal(format!("idb open: {e:?}")))?;

        // `onupgradeneeded` runs synchronously inside the open transaction —
        // it's the only place we may call `create_object_store`. We capture
        // the database from `req.result()` and create our store if missing.
        let on_upgrade = Closure::wrap(Box::new(move |evt: web_sys::Event| {
            if let Some(target) = evt.target() {
                if let Ok(req) = target.dyn_into::<IdbOpenDbRequest>() {
                    if let Ok(result) = req.result() {
                        if let Ok(db) = result.dyn_into::<IdbDatabase>() {
                            if !db.object_store_names().contains(STORE) {
                                let _ = db.create_object_store(STORE);
                            }
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        req.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));

        // Resolve the open future from `onsuccess` / `onerror`.
        let (open_tx, open_rx) = oneshot::channel::<Result<IdbDatabase, String>>();
        let open_tx = Rc::new(RefCell::new(Some(open_tx)));

        let on_success = {
            let tx = Rc::clone(&open_tx);
            Closure::wrap(Box::new(move |evt: web_sys::Event| {
                if let Some(target) = evt.target() {
                    if let Ok(r) = target.dyn_into::<IdbOpenDbRequest>() {
                        if let Ok(v) = r.result() {
                            if let Ok(db) = v.dyn_into::<IdbDatabase>() {
                                if let Some(s) = tx.borrow_mut().take() {
                                    let _ = s.send(Ok(db));
                                }
                            }
                        }
                    }
                }
            }) as Box<dyn FnMut(web_sys::Event)>)
        };
        req.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));

        let on_error = {
            let tx = Rc::clone(&open_tx);
            Closure::wrap(Box::new(move |_evt: web_sys::Event| {
                if let Some(s) = tx.borrow_mut().take() {
                    let _ = s.send(Err("open failed".into()));
                }
            }) as Box<dyn FnMut(web_sys::Event)>)
        };
        req.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        // These closures are only invoked once and after that the database
        // doesn't need them, so handing ownership to the JS GC is fine.
        on_upgrade.forget();
        on_success.forget();
        on_error.forget();

        let db = open_rx
            .await
            .map_err(|_| ClientError::Internal("idb open cancelled".into()))?
            .map_err(ClientError::Internal)?;
        Ok(Self { db })
    }
}

#[async_trait(?Send)]
impl KeyValueStore for IdbStore {
    async fn get(&self, key: &str) -> ClientResult<Option<Vec<u8>>> {
        let tx = self
            .db
            .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readonly)
            .map_err(|e| ClientError::Internal(format!("idb tx: {e:?}")))?;
        let store: IdbObjectStore = tx
            .object_store(STORE)
            .map_err(|e| ClientError::Internal(format!("idb store: {e:?}")))?;
        let req = store
            .get(&JsValue::from_str(key))
            .map_err(|e| ClientError::Internal(format!("idb get: {e:?}")))?;
        let val = await_idb_request(req).await?;
        if val.is_undefined() || val.is_null() {
            return Ok(None);
        }
        // Stored values were written as `Uint8Array`; copy back into a Vec
        // so we leave the JS heap before returning.
        let arr = js_sys::Uint8Array::new(&val);
        let mut out = vec![0u8; arr.length() as usize];
        arr.copy_to(&mut out);
        Ok(Some(out))
    }

    async fn put(&self, key: &str, value: &[u8]) -> ClientResult<()> {
        let tx = self
            .db
            .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite)
            .map_err(|e| ClientError::Internal(format!("idb tx: {e:?}")))?;
        let store = tx
            .object_store(STORE)
            .map_err(|e| ClientError::Internal(format!("idb store: {e:?}")))?;
        // `Uint8Array::from` copies into the JS heap, so the borrow of
        // `value` does not need to outlive this call.
        let arr = js_sys::Uint8Array::from(value);
        let req = store
            .put_with_key(&arr, &JsValue::from_str(key))
            .map_err(|e| ClientError::Internal(format!("idb put: {e:?}")))?;
        let _ = await_idb_request(req).await?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> ClientResult<()> {
        let tx = self
            .db
            .transaction_with_str_and_mode(STORE, IdbTransactionMode::Readwrite)
            .map_err(|e| ClientError::Internal(format!("idb tx: {e:?}")))?;
        let store = tx
            .object_store(STORE)
            .map_err(|e| ClientError::Internal(format!("idb store: {e:?}")))?;
        let req = store
            .delete(&JsValue::from_str(key))
            .map_err(|e| ClientError::Internal(format!("idb delete: {e:?}")))?;
        let _ = await_idb_request(req).await?;
        Ok(())
    }
}

/// Bridge a one-shot `IDBRequest` into an async result.
///
/// `IndexedDB` requests fire exactly one of `onsuccess` / `onerror`; we install
/// both, race them onto a oneshot channel, and let the JS GC reclaim the
/// closures after the single dispatch.
async fn await_idb_request(req: IdbRequest) -> ClientResult<JsValue> {
    let (tx, rx) = oneshot::channel::<Result<JsValue, String>>();
    let tx = Rc::new(RefCell::new(Some(tx)));
    let on_s = {
        let tx = Rc::clone(&tx);
        Closure::wrap(Box::new(move |evt: web_sys::Event| {
            if let Some(target) = evt.target() {
                if let Ok(r) = target.dyn_into::<IdbRequest>() {
                    if let Ok(v) = r.result() {
                        if let Some(s) = tx.borrow_mut().take() {
                            let _ = s.send(Ok(v));
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>)
    };
    let on_e = {
        let tx = Rc::clone(&tx);
        Closure::wrap(Box::new(move |_evt: web_sys::Event| {
            if let Some(s) = tx.borrow_mut().take() {
                let _ = s.send(Err("idb error".into()));
            }
        }) as Box<dyn FnMut(web_sys::Event)>)
    };
    req.set_onsuccess(Some(on_s.as_ref().unchecked_ref()));
    req.set_onerror(Some(on_e.as_ref().unchecked_ref()));
    on_s.forget();
    on_e.forget();
    rx.await
        .map_err(|_| ClientError::Internal("idb request cancelled".into()))?
        .map_err(ClientError::Internal)
}
