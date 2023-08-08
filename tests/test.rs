use aquaduct::*;
use url::Url;

// This is just testing the Rust backend via UniFFI exposed interfaces.
// It *does not* yet test that the Python implemented back-end can be called
// from Rust - I'm sure it can, but it's not going to be ergonomic.
#[tokio::test]
async fn test() {
    let r = Request::new(
        Method::Get,
        Url::parse("http://httpbin.org/bytes/24").unwrap(),
    );
    let resp = r.send().await.unwrap();
    let mut num = 0;
    while let Some(chunk) = resp.reader.chunk().await.unwrap() {
        num += chunk.len();
    }
    assert_eq!(num, 24);
}
