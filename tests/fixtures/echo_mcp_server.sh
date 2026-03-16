#!/bin/bash
# Minimal echo MCP server for integration tests.
# Implements MCP JSON-RPC 2.0 over stdio.
# Handles: initialize, notifications/initialized, tools/list, tools/call

while IFS= read -r line; do
    # Skip empty lines
    [[ -z "$line" ]] && continue

    # Extract method from JSON-RPC message
    method=""
    if [[ "$line" =~ \"method\"[[:space:]]*:[[:space:]]*\"([^\"]+)\" ]]; then
        method="${BASH_REMATCH[1]}"
    fi

    # Extract numeric id
    id=""
    if [[ "$line" =~ \"id\"[[:space:]]*:[[:space:]]*([0-9]+) ]]; then
        id="${BASH_REMATCH[1]}"
    fi

    case "$method" in
        initialize)
            printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"echo-test","version":"1.0.0"}}}\n' "$id"
            ;;
        notifications/initialized)
            # Notification — no response needed
            ;;
        tools/list)
            printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","description":"Echoes back the input","inputSchema":{"type":"object","properties":{"message":{"type":"string","description":"Message to echo"}}}}]}}\n' "$id"
            ;;
        tools/call)
            # Extract message value from arguments
            msg="tool called"
            if [[ "$line" =~ \"message\"[[:space:]]*:[[:space:]]*\"([^\"]*) ]]; then
                msg="${BASH_REMATCH[1]}"
                msg="${msg%%\"*}"
            fi
            printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"echo: %s"}]}}\n' "$id" "$msg"
            ;;
        *)
            # Unknown method with id — return error
            if [[ -n "$id" ]]; then
                printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$id"
            fi
            ;;
    esac
done
