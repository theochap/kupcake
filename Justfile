run-dev *args: 
    cargo build
    ./target/debug/kupcake {{args}}