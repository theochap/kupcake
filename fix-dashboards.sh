#!/usr/bin/env bash

# Fix Grafana dashboard datasource references
# Replaces ${DS_PROMETHEUS} and similar placeholders with "Prometheus"

set -e

DASHBOARDS_DIR="grafana/dashboards"

echo "Fixing datasource references in Grafana dashboards..."

# Find all JSON files and replace datasource placeholders
find "$DASHBOARDS_DIR" -name "*.json" -type f | while read -r file; do
    echo "Processing: $file"
    
    # Create backup
    cp "$file" "${file}.bak"
    
    # Replace common datasource placeholders
    sed -i 's/"datasource": "\${DS_PROMETHEUS}"/"datasource": "Prometheus"/g' "$file"
    sed -i 's/"datasource": "\$DS_PROMETHEUS"/"datasource": "Prometheus"/g' "$file"
    sed -i 's/"datasource": "default"/"datasource": "Prometheus"/g' "$file"
    sed -i 's/"datasource": "\${datasource}"/"datasource": "Prometheus"/g' "$file"
    sed -i 's/"datasource": "\$datasource"/"datasource": "Prometheus"/g' "$file"
    
    # Also fix in datasource arrays
    sed -i 's/"uid": "\${DS_PROMETHEUS}"/"uid": "Prometheus"/g' "$file"
    sed -i 's/"uid": "\$DS_PROMETHEUS"/"uid": "Prometheus"/g' "$file"
    
    echo "  ✓ Fixed: $file"
done

echo ""
echo "Done! Backups saved with .bak extension"
echo "Restart Grafana to apply changes: docker compose restart grafana"
