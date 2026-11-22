# BLE core (AND-04)

All GATT work runs on one `HandlerThread`. `BoundedTaskExecutor` caps the
number of tasks posted to it at 64; an extra request gets `BleError.QueueFull`
instead of growing an unbounded queue. Nothing in `:ble` sleeps, retries or
touches the main thread.

Each link owns an `OperationQueue` with a capacity of eight GATT steps. The
first step starts on the serial thread; MTU, discovery, descriptor writes,
writes and notifications wait for the previous callback. A non-success
callback calls `failCurrent`, which fails every waiter with the same typed
`BleError` and closes the queue. A failed GATT step ends its link under NET-03.

`LinkRegistry` registers ids allocated by Rust: the id passed to `connect`
for outbound links and the value returned by `on_accepted` for inbound ones.
It never invents or reuses an id. Its 64-entry ceiling is a memory bound, not
a link policy; Rust owns the real cap and cancels over-limit inbound links.

Tests use a manual BLE thread to prove that parallel submissions never overlap
on one link, that both bounds are enforced, and that late or duplicate link
callbacks are rejected.
