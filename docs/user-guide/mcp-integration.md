# MCP Integration

**Target Audience**: AI Assistant Users

Expose Kupcake documentation to AI assistants via the Model Context Protocol (MCP).

## What is MCP?

The [Model Context Protocol](https://modelcontextprotocol.io/) allows AI assistants to access external data sources, including local files. By exposing Kupcake docs via MCP, AI assistants can:

- Search documentation directly
- Answer questions with accurate, up-to-date information
- Reference specific configuration examples
- Help troubleshoot issues using the docs

## Setup

### Prerequisites

- **Node.js** 18+ installed
- **Claude Desktop** or another MCP-compatible AI assistant

### Configuration

#### Claude Desktop (macOS/Linux)

Edit your Claude Desktop configuration file:

**macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
**Linux**: `~/.config/Claude/claude_desktop_config.json`

Add the Kupcake docs server:

```json
{
  "mcpServers": {
    "kupcake-docs": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-filesystem",
        "/home/theo/kupcake-docs/docs"
      ]
    }
  }
}
```

**Important**: Replace `/home/theo/kupcake-docs/docs` with the absolute path to your Kupcake docs directory.

#### Claude Desktop (Windows)

Edit: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "kupcake-docs": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-filesystem",
        "C:\\Users\\YourName\\kupcake\\docs"
      ]
    }
  }
}
```

Replace with your actual path to the docs directory.

### Restart Claude Desktop

After editing the configuration:

1. **Quit** Claude Desktop completely
2. **Restart** Claude Desktop
3. **Verify** the MCP server is connected (look for connection indicator)

## Usage

Once configured, you can ask Claude questions about Kupcake and it will have direct access to the documentation:

### Example Queries

**Configuration Questions**:
```
How do I configure multiple sequencers in Kupcake?
```

**Troubleshooting**:
```
My batcher isn't submitting batches. What should I check?
```

**Architecture Questions**:
```
Explain how op-conductor coordinates sequencers.
```

**CLI Reference**:
```
What are all the environment variables I can use with Kupcake?
```

**Examples**:
```
Show me how to run Kupcake with custom Docker images.
```

## What the AI Can Access

With the filesystem MCP server, Claude can:

- ✅ Read all markdown files in `docs/`
- ✅ Search across documentation
- ✅ Reference specific sections
- ✅ Cross-reference between documents
- ✅ Read example scripts and config files

The AI **cannot**:
- ❌ Modify documentation
- ❌ Execute scripts
- ❌ Access files outside `docs/`

## Updating Documentation

The filesystem MCP server automatically reflects documentation updates:

1. Pull latest Kupcake changes:
   ```bash
   cd /path/to/kupcake
   git pull
   ```

2. Documentation is **immediately available** to the AI (no rebuild needed)

## Alternative: Read-Only Access to Entire Repository

To give the AI access to both docs and source code:

```json
{
  "mcpServers": {
    "kupcake-full": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-filesystem",
        "/home/theo/kupcake-docs"
      ]
    }
  }
}
```

This allows the AI to:
- Read documentation
- Examine source code
- Reference implementation details
- Answer questions about internals

## Multiple MCP Servers

You can configure both docs-only and full-repo access:

```json
{
  "mcpServers": {
    "kupcake-docs": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-filesystem",
        "/home/theo/kupcake-docs/docs"
      ]
    },
    "kupcake-source": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-filesystem",
        "/home/theo/kupcake-docs/crates"
      ]
    }
  }
}
```

## Troubleshooting

### MCP Server Not Connecting

**Check Node.js is installed**:
```bash
node --version
# Should show v18.0.0 or higher
```

**Check configuration file syntax**:
```bash
# Validate JSON
cat ~/Library/Application\ Support/Claude/claude_desktop_config.json | jq
```

**Check path is correct**:
```bash
ls /home/theo/kupcake-docs/docs/README.md
# Should exist and be readable
```

### Permission Denied

Ensure the docs directory is readable:

```bash
chmod -R +r /home/theo/kupcake-docs/docs
```

### Changes Not Reflected

The filesystem MCP server reads files on-demand, so changes are immediate. If not working:

1. Verify you saved the documentation file
2. Restart Claude Desktop
3. Try asking a specific question about the new content

## Security Considerations

### Read-Only Access

The filesystem MCP server provides **read-only** access. It cannot:
- Modify files
- Delete files
- Execute commands

### Path Restrictions

The server can only access:
- The specific directory configured
- Subdirectories within that path

To restrict access further, configure only specific subdirectories:

```json
{
  "mcpServers": {
    "kupcake-getting-started": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-filesystem",
        "/home/theo/kupcake-docs/docs/getting-started"
      ]
    }
  }
}
```

## Benefits

### Always Up-to-Date

Unlike copying docs into prompts:
- No manual updates needed
- Always references latest documentation
- Automatically includes new guides

### Efficient Context Usage

The AI only reads relevant documentation:
- Doesn't load entire docs into context
- Searches for specific information
- More efficient token usage

### Better Answers

Direct doc access provides:
- Accurate configuration examples
- Current CLI arguments
- Latest troubleshooting steps
- Runnable example code

## Related Documentation

- [MCP Official Documentation](https://modelcontextprotocol.io/)
- [Filesystem MCP Server](https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem)
- [Getting Started Guide](../getting-started/quickstart.md)
- [CLI Reference](cli-reference.md)

## Example Configuration File

Complete example for `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "kupcake-docs": {
      "command": "npx",
      "args": [
        "-y",
        "@modelcontextprotocol/server-filesystem",
        "/absolute/path/to/kupcake/docs"
      ]
    }
  },
  "globalShortcut": "Ctrl+Space"
}
```

Remember to:
1. Replace path with your actual Kupcake location
2. Use absolute paths (not `~` or relative paths)
3. Restart Claude Desktop after saving
