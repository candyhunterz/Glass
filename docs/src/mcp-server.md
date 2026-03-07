# MCP Server

Glass includes a built-in [Model Context Protocol](https://modelcontextprotocol.io/) (MCP) server, allowing AI assistants to interact with your terminal history, undo system, and other Glass features.

## What is MCP?

The Model Context Protocol is an open standard for connecting AI assistants to external tools and data sources. Glass acts as an MCP server, exposing your terminal context to any MCP-compatible client.

## Setting up with Claude Desktop

To connect Glass to Claude Desktop:

1. Open Claude Desktop settings.
2. Navigate to the MCP servers configuration.
3. Add Glass as an MCP server:

```json
{
  "mcpServers": {
    "glass": {
      "command": "glass",
      "args": ["mcp"]
    }
  }
}
```

4. Restart Claude Desktop to connect.

## Available capabilities

Glass exposes the following through MCP:

### Tools

- **Command history queries** -- Search and retrieve past commands, their output, exit codes, and timing information.
- **Undo operations** -- Trigger file undo to restore files modified by recent commands.

### Resources

- **Session context** -- Current working directory, recent commands, and active shell information.
- **Command output** -- Access the output of specific commands by reference.

## Other MCP clients

Any MCP-compatible AI assistant can connect to Glass. The setup is similar across clients:

1. Point the client to the `glass mcp` command.
2. The client will discover available tools and resources automatically.
3. The AI assistant can then query your terminal history, inspect command output, and trigger undo operations as part of its workflow.

## Privacy

MCP access is local only. Glass does not send terminal data to any external service. The MCP server runs on your machine, and only locally connected clients can access it.
