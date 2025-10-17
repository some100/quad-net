//! Async http requests.

#[cfg(target_arch = "wasm32")]
use sapp_jsutils::JsObject;

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum Method {
    Post,
    Put,
    Get,
    Delete,
}

#[derive(Debug)]
pub enum HttpError {
    IOError,
    NotStrError,
    #[cfg(not(target_arch = "wasm32"))]
    UreqError(ureq::Error),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::IOError => write!(f, "IOError"),
            HttpError::NotStrError => write!(f, "Received bytes that were not a string"),
            #[cfg(not(target_arch = "wasm32"))]
            HttpError::UreqError(error) => write!(f, "Ureq error: {error}"),
        }
    }
}
impl From<std::io::Error> for HttpError {
    fn from(_error: std::io::Error) -> HttpError {
        HttpError::IOError
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<ureq::Error> for HttpError {
    fn from(error: ureq::Error) -> HttpError {
        HttpError::UreqError(error)
    }
}

#[cfg(target_arch = "wasm32")]
extern "C" {
    fn http_make_request(scheme: i32, url: JsObject, body: JsObject, headers: JsObject) -> i32;
    fn http_try_recv(cid: i32) -> JsObject;
}

#[cfg(not(target_arch = "wasm32"))]
pub struct Request {
    rx: std::sync::mpsc::Receiver<Result<Vec<u8>, HttpError>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Request {
    pub fn try_recv_str(&mut self) -> Option<Result<String, HttpError>> {
        match self.rx.try_recv() {
            Ok(Ok(res)) => Some(String::from_utf8(res).map_err(|_| HttpError::NotStrError)),
            Ok(Err(e)) => Some(Err(e)),
            Err(_) => None,
        }
    }

    pub fn try_recv_bytes(&mut self) -> Option<Vec<u8>> {
        Some(self.rx.try_recv().ok()?.ok()?)
    }
}

#[cfg(target_arch = "wasm32")]
pub struct Request {
    cid: i32,
}

#[cfg(target_arch = "wasm32")]
impl Request {
    pub fn try_recv_str(&mut self) -> Option<Result<String, HttpError>> {
        let js_obj = unsafe { http_try_recv(self.cid) };

        if js_obj.is_nil() == false {
            let mut buf = vec![];
            js_obj.to_byte_buffer(&mut buf);

            let res = String::from_utf8(buf).map_err(|_| HttpError::NotStrError);
            Some(res)
        } else {
            None
        }
    }

    pub fn try_recv_bytes(&mut self) -> Option<Vec<u8>> {
        let js_obj = unsafe { http_try_recv(self.cid) };

        if js_obj.is_nil() == false {
            let mut buf = vec![];
            js_obj.to_byte_buffer(&mut buf);

            Some(buf)
        } else {
            None
        }
    }
}

pub struct RequestBuilder {
    url: String,
    method: Method,
    headers: Vec<(String, String)>,
    query: Vec<(String, String)>,
    body: Option<String>,
}

impl RequestBuilder {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            method: Method::Get,
            headers: vec![],
            query: vec![],
            body: None,
        }
    }

    pub fn method(self, method: Method) -> Self {
        Self { method, ..self }
    }

    pub fn header(mut self, header: &str, value: &str) -> Self {
        self.headers.push((header.to_owned(), value.to_owned()));

        Self {
            headers: self.headers,
            ..self
        }
    }

    pub fn query(mut self, key: &str, value: &str) -> Self {
        self.query.push((key.to_owned(), value.to_owned()));

        Self {
            query: self.query,
            ..self
        }
    }

    pub fn body(self, body: &str) -> Self {
        Self {
            body: Some(body.to_owned()),
            ..self
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn send(self) -> Request {
        use std::sync::mpsc::channel;

        let (tx, rx) = channel();

        std::thread::spawn(move || {
            let mut request = match self.method {
                Method::Post => ureq::post(&self.url),
                Method::Put => ureq::put(&self.url),
                Method::Get => ureq::get(&self.url).force_send_body(),
                Method::Delete => ureq::delete(&self.url).force_send_body(),
            };

            for (header, value) in self.headers {
                request = request.header(header, value);
            }

            for (key, value) in self.query {
                request = request.query(key, value);
            }

            let response: Result<_, HttpError> = if let Some(body) = self.body {
                request.send(&body)
            } else {
                request.send_empty()
            }
            .map_err(|err| err.into())
            .and_then(|response| response.into_body().read_to_vec().map_err(|err| err.into()));

            tx.send(response).unwrap();
        });

        Request { rx }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn send(&self) -> Request {
        let scheme = match self.method {
            Method::Post => 0,
            Method::Put => 1,
            Method::Get => 2,
            Method::Delete => 3,
        };

        let headers = JsObject::object();

        for (header, value) in &self.headers {
            headers.set_field_string(&header, &value);
        }

        let mut url = self.url.clone();

        if !self.query.is_empty() {
            let query = self
                .query
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<String>>()
                .join("&");

            url = format!("{url}?{query}");
        }

        let cid = unsafe {
            http_make_request(
                scheme,
                JsObject::string(&url),
                JsObject::string(&self.body.as_ref().map(|s| s.as_str()).unwrap_or("")),
                headers,
            )
        };
        Request { cid }
    }
}
