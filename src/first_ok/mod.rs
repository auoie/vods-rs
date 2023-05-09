use std::sync::{Arc, Mutex};

use futures::Future;
use tokio::{select, sync::mpsc};

async fn send_item_requests<Iter, T>(items: Iter, item_sender: async_channel::Sender<T>)
where
    Iter: ExactSizeIterator<Item = T>,
{
    for item in items {
        let send_response = item_sender.send(item).await;
        if send_response.is_err() {
            return;
        }
    }
}

async fn process_item_requests<I, T, E, F, Fut>(
    item_receiver: async_channel::Receiver<I>,
    checker: F,
    response_sender: mpsc::Sender<Result<T, E>>,
) where
    F: (FnOnce(I) -> Fut) + Copy,
    Fut: Future<Output = Result<T, E>>,
{
    while let Ok(item) = item_receiver.recv().await {
        select! {
            result = checker(item) => {
                let _ = response_sender.send(result).await;
            }
            _ = response_sender.closed() => {
                return;
            }
        }
    }
}

async fn process_item_responses<T, E>(
    length: usize,
    mut response_receiver: mpsc::Receiver<Result<T, E>>,
) -> Option<Result<T, E>> {
    let mut result: Option<Result<T, E>> = None;
    for _ in 0..length {
        let option_result_cur = response_receiver.recv().await;
        match option_result_cur {
            Some(result_cur) => match result_cur {
                Ok(_) => {
                    result = Some(result_cur);
                    break;
                }
                Err(_) => {
                    result = Some(result_cur);
                }
            },
            None => {
                break;
            }
        }
    }
    result
}

/// Returns the first non-error result from a function `checker` applied to each entry in a list of `items`.
/// If the list of items is empty, it returns `None`.
/// If all of the results are errors, it returns the last error.
/// There are `concurrent` workers to apply the `checker` function.
/// If `concurrent` is 0, then it will create `len(items)` workers.
pub async fn get_first_ok_bounded<Iter, I, T, E, F, Fut>(
    items: Iter,
    mut concurrent: usize,
    checker: F,
) -> Option<Result<T, E>>
where
    F: (FnOnce(I) -> Fut) + Send + Copy + 'static,
    Fut: Future<Output = Result<T, E>> + Send,
    I: Send + 'static,
    T: Send + 'static,
    E: Send + 'static,
    Iter: ExactSizeIterator<Item = I> + Send + 'static,
{
    let length = items.len();
    if concurrent == 0 {
        concurrent = length;
    }
    let (item_sender, item_receiver) = async_channel::bounded::<I>(1);
    let (response_sender, response_receiver) = mpsc::channel::<Result<T, E>>(1);
    let count = Arc::new(Mutex::new(0));
    for _ in 0..concurrent {
        let item_receiver = item_receiver.clone();
        let response_sender = response_sender.clone();
        let count = count.clone();
        tokio::task::spawn(async move {
            // {
            //     let mut locked = count.lock().unwrap();
            //     *locked += 1;
            //     println!("started {}", locked);
            // }
            process_item_requests(item_receiver, checker, response_sender).await;
            {
                let mut locked = count.lock().unwrap();
                *locked += 1;
                println!("started {}", locked);
            }
        });
    }
    tokio::task::spawn(async move {
        send_item_requests(items, item_sender).await;
    });
    process_item_responses(length, response_receiver).await
}
