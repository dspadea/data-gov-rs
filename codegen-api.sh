#!/bin/bash

# Check if the YAML file exists
if [ ! -f "ckan-openapi-spec.yaml" ]; then
    echo "Error: ckan-openapi-spec.yaml not found in current directory"
    echo "Current directory: $(pwd)"
    echo "Files in directory:"
    ls -la
    exit 1
fi

echo "Found ckan-openapi-spec.yaml, proceeding with code generation..."

# Use podman instead of docker with :Z flag for SELinux
podman run --rm -v "${PWD}:/local:Z" openapitools/openapi-generator-cli generate \
    --additional-properties packageName=data-gov-ckan,library=reqwest-trait,topLevelApiClient=true \
    -i /local/ckan-openapi-spec.yaml \
    -g rust \
    -o /local/data-gov-ckan 