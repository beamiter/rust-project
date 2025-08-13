RUSTFLAGS="-Z sanitizer=address" cargo test --features use-eventfd --target x86_64-unknown-linux-gnu
RUSTFLAGS="-Z sanitizer=thread" cargo test --features use-eventfd --target x86_64-unknown-linux-gnu
RUSTFLAGS="-Z sanitizer=memory" cargo test --features use-eventfd --target x86_64-unknown-linux-gnu

RUSTFLAGS="-Z sanitizer=address" cargo test --features use-semaphore --target x86_64-unknown-linux-gnu
RUSTFLAGS="-Z sanitizer=thread" cargo test --features use-semaphore --target x86_64-unknown-linux-gnu
RUSTFLAGS="-Z sanitizer=memory" cargo test --features use-semaphore --target x86_64-unknown-linux-gnu
