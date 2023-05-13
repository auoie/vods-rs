# Notes

## Vec Ownership

There is no way to get an owned subset of the the indices of a `Vec`.
For example, if you have the indicies `[2, 3, 5, 7, 11]`,
there is no way to get those 5 elements as owned values.

I need this in order to use the `get_sorted_indices_of_valid_urls` function.

See here to iterate over the odd-indexed elements of a `Vec` in Rust.

## Scoped Tasks

- https://tmandry.gitlab.io/blog/posts/2023-03-01-scoped-tasks/
- https://internals.rust-lang.org/t/blog-post-a-formulation-for-scoped-tasks/18448
- https://conradludgate.com/posts/async-stack

There is `std::thread::scope` and `crossbeam_utils::thread::scope`.
It would be nice for there to be a scoped tokio task so that I don't need ownership of stuff to use `first_ok::get_first_ok_bounded`.
It is possible to do it unsafely with `async_scoped::scope_and_collect`.

## DNS Blocking

I was getting an issue with the StreamsCharts command where it would just stall for around 10 seconds.
In order to debug the issue, I was led to [this Reddit comment](https://www.reddit.com/r/rust/comments/t3jy9t/comment/hytx8kv/?utm_source=share&utm_medium=web2x&context=3).
So I used [tokio-console](https://github.com/tokio-rs/console).
This showed the following

```
hyper-0.14.26/src/client/connect/dns.rs
hyper::client::connect::dns::GaiResolver as tower_service::Service
this task has lost its waker and will never be awoken again
```

In particular, the DNS resolver for `hyper` uses `tokio::task::spawn_blocking`.
See [here](https://github.com/hyperium/hyper/issues/977) and [here](https://users.rust-lang.org/t/dns-error-when-sending-1024-parallel-requests-with-reqwest/91615) for similar issues.

It seems like it uses `getaddrinfo` internally which is a [blocking call](https://skarnet.org/software/s6-dns/getaddrinfo.html).
See [here](https://github.com/tokio-rs/mio/issues/668) for a GitHub comment.

The solution is to instead use the `trust-dns` async resolver which is a feature in the `reqwest` crate.

## Robust HTTP Client

To test the robustness of the HTTP client, use the `--filter-invalid 100` option and then turn on a VPN while it is running.
It should pause for the timeout duration and then continue as normal.
When I use the `.use_rustls_tls()` option, it fails on the intial request and the retry, taking twice as long.
When I don't use the `rustls` TLS backend, it just times out once and then proceeds as normal.

[This person](https://www.reddit.com/r/rust/comments/oir1g1/comment/h4yn4kw/?utm_source=share&utm_medium=web2x&context=3) suggests that async is the right choice for a server but the wrong choice for a client.

Upon some further testing, `ureq` with `rustls` doesn't seem to have any problems.
So I just copied their `rustls::ClientConfig`.

```rust
fn root_certs() -> rustls::RootCertStore {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
        rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
            ta.subject,
            ta.spki,
            ta.name_constraints,
        )
    }));
    root_store
}

fn make_tls() -> rustls::ClientConfig {
    let tls: rustls::ConfigBuilder<
        rustls::ClientConfig,
        rustls::client::WantsTransparencyPolicyOrClientCert,
    > = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_certs());
    tls.with_no_client_auth()
}

fn make_robust_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .use_preconfigured_tls(make_tls())
        .trust_dns(true)
        .build()
}
```

Comparing with `reqwest` and doing some tests showed that the main offender seems to be

```
tls.alpn_protocols = vec!["h2".into(), "http/1.1".into()];
```

If I add that to my `ClientConfig`, then it continues to fail.
Any easier solution might just to to use `.use_rustls_tls().http1_only()` since `.use_preconfigured_tls(make_tls())` requires that I keep my `rustls` version that of the `reqwest` crate.

I'm still not sure how to replicate the following in `rust`.

```go
func makeRobustClient() *http.Client {
	timeout := 10 * time.Second
	dialer := &net.Dialer{
		Timeout: timeout,
	}
	return &http.Client{
		Timeout:   timeout,
		Transport: &http.Transport{DialContext: dialer.DialContext},
	}
}
```

You could instead do

```rust
fn make_robust_client() -> Result<Client, reqwest::Error> {
    Client::builder()
        .timeout(Duration::from_secs(5))
        .http2_keep_alive_timeout(Duration::from_millis(500))
        .http2_keep_alive_interval(Duration::from_millis(250))
        .http2_keep_alive_while_idle(true)
        .http2_adaptive_window(true)
        .use_rustls_tls()
        // .http2_prior_knowledge()
        .trust_dns(true)
        .build()
}
```

The method call `.http2_adaptive_window(true)` makes HTTP/2 much faster than HTTP1/1.

It won't stall (but very rarely it still does for some reason).
But a lot of the segments will fail.
In order to make them succeed, I needed to add a delay on the `retry_on_error` function.
In practice, for this particular, binary, the retry function is only called when a VPN is turned on or something with NAT happens.
