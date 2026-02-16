run-dev *args:
    cargo build
    ./target/debug/kupcake {{args}}

# Run unit tests
test:
    cargo test --lib --bins

# Run integration tests (concurrency limited by in-code semaphore)
test-integration *args:
    cargo test --test integration_test -- {{args}}

# Run health check against a deployed network
health config:
    cargo run -- health {{config}}

# Kill all kupcake networks and containers
kill-all:
    @echo "Stopping all kupcake containers..."
    @docker ps -q --filter "name=kup-" | xargs -r docker stop
    @echo "Removing all kupcake containers..."
    @docker ps -aq --filter "name=kup-" | xargs -r docker rm -f
    @echo "Removing all kupcake networks..."
    @docker network ls --filter "name=kup-" -q | xargs -r docker network rm -f
    @echo "✓ All kupcake networks cleaned up!"
