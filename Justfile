run-dev *args:
    cargo build
    ./target/debug/kupcake {{args}}

# Kill all kupcake networks and containers
kill-all:
    @echo "Stopping all kupcake containers..."
    @docker ps -q --filter "name=kup-" | xargs -r docker stop
    @echo "Removing all kupcake containers..."
    @docker ps -aq --filter "name=kup-" | xargs -r docker rm
    @echo "Removing all kupcake networks..."
    @docker network ls --filter "name=kup-" -q | xargs -r docker network rm
    @echo "✓ All kupcake networks cleaned up!"