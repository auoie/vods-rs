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
