#!/usr/bin/env bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

check_jwt() {
    if [[ ! -f "jwttoken/jwt.hex" ]]; then
        print_warning "JWT secret not found. Generating..."
        ./generate-jwt.sh
        print_success "JWT secret generated"
    else
        print_success "JWT secret exists"
    fi
}

check_env() {
    local missing=0
    
    if [[ -z "${L1_PROVIDER_RPC}" ]]; then
        print_error "L1_PROVIDER_RPC is not set"
        missing=1
    fi
    
    if [[ -z "${L1_BEACON_API}" ]]; then
        print_error "L1_BEACON_API is not set"
        missing=1
    fi
    
    if [[ $missing -eq 1 ]]; then
        print_info "Please set the required environment variables:"
        echo ""
        echo "  export L1_PROVIDER_RPC='https://ethereum-sepolia-rpc.publicnode.com'"
        echo "  export L1_BEACON_API='https://ethereum-sepolia-beacon-api.publicnode.com'"
        echo ""
        echo "Or create a .env file with these variables."
        exit 1
    fi
    
    print_success "Required environment variables are set"
}

show_status() {
    print_info "Docker Compose Status:"
    docker compose ps
}

show_logs() {
    local service="${1:-}"
    if [[ -n "$service" ]]; then
        print_info "Following logs for $service..."
        docker compose logs -f "$service"
    else
        print_info "Following logs for all services..."
        docker compose logs -f
    fi
}

show_config() {
    print_info "Current Configuration:"
    echo ""
    echo "Images:"
    echo "  Op-Reth: ${OP_RETH_IMAGE:-ghcr.io/paradigmxyz/op-reth}:${OP_RETH_TAG:-latest}"
    echo "  Kona:    ${KONA_IMAGE:-ghcr.io/op-rs/kona/kona-node}:${KONA_TAG:-latest}"
    echo ""
    echo "Chain:"
    echo "  Op-Reth: ${OP_RETH_CHAIN:-optimism-sepolia}"
    echo "  Kona:    ${KONA_CHAIN:-optimism-sepolia}"
    echo ""
    echo "L1 Endpoints:"
    echo "  RPC:    ${L1_PROVIDER_RPC:-<not set>}"
    echo "  Beacon: ${L1_BEACON_API:-<not set>}"
    echo ""
    echo "Ports:"
    echo "  Op-Reth RPC:        ${OP_RETH_RPC_PORT:-8545}"
    echo "  Op-Reth Engine:     ${OP_RETH_ENGINE_PORT:-8551}"
    echo "  Op-Reth Metrics:    ${OP_RETH_METRICS_PORT:-9001}"
    echo "  Kona RPC:           ${KONA_RPC_PORT:-5060}"
    echo "  Kona Metrics:       ${KONA_METRICS_PORT:-9002}"
    echo ""
}

show_help() {
    cat << EOF
Op-Reth + Kona Node Stack Management Script

Usage: $0 [command] [options]

Commands:
    up [tag]           Start the stack (optionally with specific tag)
                       Examples:
                         $0 up
                         $0 up v1.2.3
                         $0 up OP_RETH_TAG=v1.0.0 KONA_TAG=v0.5.0

    down               Stop the stack (keeps data)
    down -v            Stop the stack and remove all data volumes
    
    restart            Restart the stack
    
    logs [service]     Show logs (optionally for specific service)
                       Examples:
                         $0 logs
                         $0 logs op-reth
                         $0 logs kona-node
    
    status             Show status of all services
    
    config             Show current configuration
    
    shell <service>    Open a shell in a service container
                       Examples:
                         $0 shell op-reth
                         $0 shell kona-node
    
    test-rpc           Test the Op-Reth RPC endpoint
    
    metrics            Show metrics URLs
    
    jwt                Generate/regenerate JWT secret
    
    pull               Pull latest images
    
    clean              Stop services and remove all data (use with caution!)
    
    help               Show this help message

Environment Variables:
    L1_PROVIDER_RPC    L1 execution RPC endpoint (required)
    L1_BEACON_API      L1 beacon API endpoint (required)
    OP_RETH_TAG        Op-Reth image tag (default: latest)
    KONA_TAG           Kona-Node image tag (default: latest)
    OP_RETH_CHAIN      Chain to sync (default: optimism-sepolia)
    KONA_CHAIN         Chain to sync (default: optimism-sepolia)

Examples:
    # Start with latest versions
    export L1_PROVIDER_RPC="https://ethereum-sepolia-rpc.publicnode.com"
    export L1_BEACON_API="https://ethereum-sepolia-beacon-api.publicnode.com"
    $0 up

    # Start with specific versions
    OP_RETH_TAG=v1.2.3 KONA_TAG=v0.5.0 $0 up

    # View logs
    $0 logs op-reth

    # Check status
    $0 status

EOF
}

cmd_up() {
    check_jwt
    check_env
    
    print_info "Starting stack..."
    docker compose up -d
    print_success "Stack started"
    echo ""
    show_status
}

cmd_down() {
    local extra_args=""
    if [[ "$1" == "-v" ]]; then
        print_warning "This will remove all data volumes!"
        read -p "Are you sure? (y/N) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            print_info "Cancelled"
            exit 0
        fi
        extra_args="-v"
    fi
    
    print_info "Stopping stack..."
    docker compose down $extra_args
    print_success "Stack stopped"
}

cmd_restart() {
    print_info "Restarting stack..."
    docker compose restart
    print_success "Stack restarted"
    show_status
}

cmd_shell() {
    local service="$1"
    if [[ -z "$service" ]]; then
        print_error "Please specify a service: op-reth, kona-node, prometheus, or grafana"
        exit 1
    fi
    print_info "Opening shell in $service..."
    docker compose exec "$service" /bin/bash
}

cmd_test_rpc() {
    print_info "Testing Op-Reth RPC endpoint..."
    local port="${OP_RETH_RPC_PORT:-8545}"
    curl -s -X POST "http://localhost:${port}" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq .
    
    if [[ $? -eq 0 ]]; then
        print_success "RPC endpoint is responding"
    else
        print_error "RPC endpoint is not responding"
        exit 1
    fi
}

cmd_metrics() {
    local op_reth_port="${OP_RETH_METRICS_PORT:-9001}"
    local kona_port="${KONA_METRICS_PORT:-9002}"
    local prometheus_port="${PROMETHEUS_PORT:-9091}"
    
    print_info "Metrics endpoints:"
    echo ""
    echo "  Op-Reth:    http://localhost:${op_reth_port}/metrics"
    echo "  Kona-Node:  http://localhost:${kona_port}/metrics"
    echo "  Prometheus: http://localhost:${prometheus_port}"
    echo ""
}

cmd_pull() {
    print_info "Pulling latest images..."
    docker compose pull
    print_success "Images updated"
}

cmd_clean() {
    print_warning "This will STOP ALL SERVICES and REMOVE ALL DATA!"
    read -p "Are you sure? Type 'yes' to confirm: " -r
    echo
    if [[ $REPLY == "yes" ]]; then
        print_info "Cleaning up..."
        docker compose down -v
        print_success "Cleanup complete"
    else
        print_info "Cancelled"
    fi
}

# Main command dispatcher
case "${1:-help}" in
    up)
        shift
        cmd_up "$@"
        ;;
    down)
        shift
        cmd_down "$@"
        ;;
    restart)
        cmd_restart
        ;;
    logs)
        shift
        show_logs "$@"
        ;;
    status)
        show_status
        ;;
    config)
        show_config
        ;;
    shell)
        shift
        cmd_shell "$@"
        ;;
    test-rpc)
        cmd_test_rpc
        ;;
    metrics)
        cmd_metrics
        ;;
    jwt)
        ./generate-jwt.sh
        ;;
    pull)
        cmd_pull
        ;;
    clean)
        cmd_clean
        ;;
    help|--help|-h)
        show_help
        ;;
    *)
        print_error "Unknown command: $1"
        echo ""
        show_help
        exit 1
        ;;
esac

