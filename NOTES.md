# Notes

## Vec Ownership

There is no way to get an owned subset of the the indicies of a `Vec`.
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
