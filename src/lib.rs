use parking_lot::Mutex;
use std::cell::Cell;
use std::sync::Arc;
use url::Url;

#[derive(Debug, uniffi::Error, thiserror::Error)]
#[uniffi(flat_error)]
pub enum Error {
    #[error("oops")]
    Oops,
    #[error("reqwest error: {0:?}")]
    RequestError(#[from] reqwest::Error),
}

// Use `url::Url` as a custom type, with `String` as the Builtin
::uniffi::custom_type!(Url, String);
impl UniffiCustomTypeConverter for Url {
    type Builtin = String;

    fn into_custom(val: Self::Builtin) -> uniffi::Result<Self> {
        Ok(Url::parse(&val)?)
    }

    fn from_custom(obj: Self) -> Self::Builtin {
        obj.into()
    }
}

#[derive(uniffi::Enum, Clone, Debug, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
#[repr(u8)]
pub enum Method {
    Get,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(uniffi::Object, Debug)]
pub struct Request {
    url: Url,
    method: Method,
}

// ===============================================================================
// This section is what a "backend" implemented in Rust might look like.
// It can be consumed by either Rust or Foreign code.
//
// However, this is *not* what viaducts does - it needs the *backend* to be either
// Rust or Foreign - the caller is always Rust.
//
// IOW, this is more a demo than helping us get to where viaduct is.
#[uniffi::export(async_runtime = "tokio")]
impl Request {
    /// Construct a new request to the given `url` using the given `method`.
    /// Note that the request is not made until `send()` is called.
    #[uniffi::constructor]
    pub fn new(method: Method, url: Url) -> Arc<Self> {
        Arc::new(Self { url, method })
    }

    pub async fn send(&self) -> Result<Response, Error> {
        let res = reqwest::get(self.url.clone()).await?;
        let status = res.status().as_u16();
        let reader = Arc::new(Reader::new(res));
        let res = {
            Response {
                request_method: self.method,
                url: self.url.clone(),
                status,
                reader,
            }
        };
        Ok(res)
    }
}

/// A response from the server.
#[derive(uniffi::Record, Clone)]
pub struct Response {
    /// The method used to request this response.
    pub request_method: Method,
    /// The URL of this response.
    pub url: Url,
    /// The HTTP Status code of this response.
    pub status: u16,

    pub reader: Arc<Reader>,
}

/// A "Reader" can get chunks from the server asynchronously.
#[derive(uniffi::Object)]
pub struct Reader {
    resp: Mutex<Cell<Option<reqwest::Response>>>,
}

impl Reader {
    fn new(resp: reqwest::Response) -> Self {
        Self {
            resp: Mutex::new(Cell::new(Some(resp))),
        }
    }
}

#[uniffi::export]
impl Reader {
    pub async fn chunk(&self) -> Result<Option<Vec<u8>>, Error> {
        let mut resp = {
            let lock = self.resp.lock();
            lock.replace(None).expect("multiple readers?")
        };
        let chunk = resp.chunk().await?;
        self.resp.lock().replace(Some(resp));
        Ok(chunk.map(|b| b.to_vec()))
    }
}

// ===============================================================================
// Everything below here is an attempt at getting things working in the other
// direction - ie, with a 'backend' implemented on the foreign side.
//
// Note that this would probably be shaped much better if we had async callbacks/traits
static BACKEND: std::sync::OnceLock<Box<dyn Backend>> = std::sync::OnceLock::new();

// Ideally we could use the `Response` etc types directly, but that requires 2 implementations
// (ie, one for a Rust backend, one for a foreign backend) which implies traits, but we
// don't handle async traits just yet.
#[uniffi::export]
fn set_backend(b: Box<dyn Backend>) -> Result<(), Error> {
    BACKEND.set(b).map_err(|_| Error::Oops)
}

#[uniffi::export(callback_interface)]
pub trait Backend: Send + Sync {
    fn start(&self, recv: Arc<ForeignReceiver>);
}

// This object "receives" the data from the foreign side, then sends it
// over a channel to the ultimate consumer.
#[derive(uniffi::Object)]
pub struct ForeignReceiver {
    sender: async_channel::Sender<Vec<u8>>,
}

impl ForeignReceiver {
    fn new(sender: async_channel::Sender<Vec<u8>>) -> Self {
        Self { sender }
    }
}

#[uniffi::export]
impl ForeignReceiver {
    fn on_data_available(&self, data: Vec<u8>) {
        self.sender.send_blocking(data).unwrap();
    }

    fn done(&self) {
        self.sender.close();
    }
}

// A struct which can take data from the foreign side and turn it into an async reader.
#[derive(uniffi::Object)]
pub struct BytesReader {
    recv: async_channel::Receiver<Vec<u8>>,
}

#[uniffi::export]
impl BytesReader {
    pub async fn chunk(&self) -> Result<Option<Vec<u8>>, Error> {
        Ok(match self.recv.recv().await {
            Ok(data) => Some(data),
            Err(_) => None,
        })
    }
}

// How the foreign side (or technically the Rust side too) kicks off the process.
#[uniffi::export]
fn new_request_reader() -> Result<Arc<BytesReader>, Error> {
    let (s, r) = async_channel::unbounded();
    let foreign_recv = Arc::new(ForeignReceiver::new(s));
    BACKEND.get().unwrap().start(foreign_recv);
    Ok(Arc::new(BytesReader { recv: r }))
}

uniffi::setup_scaffolding!();
