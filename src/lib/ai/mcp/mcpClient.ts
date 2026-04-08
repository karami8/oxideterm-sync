// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

/**
 * MCP Client — Handles communication with MCP servers
 * 
 * Supports two transports:
 * - SSE (HTTP): Direct HTTP requests from frontend
 * - Stdio: JSON-RPC over stdin/stdout, managed by Rust backend
 */

import { api } from '../../api';
import type {
  McpServerState,
  McpToolSchema,
  McpResource,
  McpResourceContent,
  McpCallToolResult,
  JsonRpcRequest,
  JsonRpcResponse,
  McpServerCapabilities,
} from './mcpTypes';

class McpHttpStatusError extends Error {
  constructor(public readonly status: number, public readonly statusText: string) {
    super(`MCP SSE request failed: ${status} ${statusText}`);
  }
}

type McpSseEvent = {
  event: string;
  data: string;
};

type HttpRequestResult = {
  endpointUrl: string;
  sessionId?: string;
  response?: JsonRpcResponse;
};

let nextRequestId = 1;

function makeRequest(method: string, params?: Record<string, unknown>): JsonRpcRequest {
  return { jsonrpc: '2.0', id: nextRequestId++, method, params };
}

function makeNotification(method: string, params?: Record<string, unknown>): Omit<JsonRpcRequest, 'id'> & { id?: undefined } {
  return { jsonrpc: '2.0', method, params } as Omit<JsonRpcRequest, 'id'> & { id?: undefined };
}

/**
 * Retrieve the auth token for an MCP server from the OS keychain.
 * Falls back to the legacy `authToken` field on config for migration.
 */
async function getMcpAuthToken(config: { id: string; authToken?: string }): Promise<string | undefined> {
  try {
    const keychainToken = await api.getAiProviderApiKey(`mcp:${config.id}`);
    if (keychainToken) return keychainToken;
  } catch {
    // keychain access failed — fall through to legacy
  }
  if (config.authToken) {
    console.info(`[MCP] Using legacy authToken for ${config.id} — migrate to keychain`);
    return config.authToken;
  }
  return undefined;
}

/**
 * Store an MCP server auth token in the OS keychain.
 */
export async function setMcpAuthToken(serverId: string, token: string): Promise<void> {
  await api.setAiProviderApiKey(`mcp:${serverId}`, token);
}

/**
 * Delete an MCP server auth token from the OS keychain.
 */
export async function deleteMcpAuthToken(serverId: string): Promise<void> {
  await api.deleteAiProviderApiKey(`mcp:${serverId}`);
}

// ═══════════════════════════════════════════════════════════════════════════
// SSE Transport
// ═══════════════════════════════════════════════════════════════════════════

function validateMcpUrl(urlStr: string): URL {
  const parsed = new URL(urlStr);
  if (!['http:', 'https:'].includes(parsed.protocol)) {
    throw new Error('MCP SSE only supports http/https URLs');
  }
  return parsed;
}

function resolveSseMessageUrl(baseUrl: string): string {
  const base = validateMcpUrl(baseUrl);
  const pathname = base.pathname.replace(/\/+$/, '');
  if (!pathname) {
    return new URL('/message', base).href;
  }
  if (pathname.endsWith('/message')) {
    return base.href;
  }
  if (pathname.endsWith('/sse')) {
    const next = new URL(base.href);
    next.pathname = `${pathname.slice(0, -4)}/message`;
    return next.href;
  }
  return base.href;
}

async function readSseEvents(resp: Response): Promise<McpSseEvent[]> {
  const body = resp.body;
  if (!body) {
    return [];
  }

  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  const events: McpSseEvent[] = [];
  let eventName = 'message';
  let dataLines: string[] = [];

  const flushEvent = () => {
    if (dataLines.length === 0) {
      eventName = 'message';
      return;
    }
    events.push({ event: eventName, data: dataLines.join('\n') });
    eventName = 'message';
    dataLines = [];
  };

  while (true) {
    const { done, value } = await reader.read();
    buffer += decoder.decode(value, { stream: !done });

    let boundary = buffer.indexOf('\n');
    while (boundary !== -1) {
      let line = buffer.slice(0, boundary);
      buffer = buffer.slice(boundary + 1);
      if (line.endsWith('\r')) {
        line = line.slice(0, -1);
      }

      if (!line) {
        flushEvent();
      } else if (line.startsWith(':')) {
        // Comment line — ignore.
      } else if (line.startsWith('event:')) {
        eventName = line.slice(6).trim() || 'message';
      } else if (line.startsWith('data:')) {
        dataLines.push(line.slice(5).trimStart());
      }

      boundary = buffer.indexOf('\n');
    }

    if (done) {
      if (buffer.trim()) {
        if (buffer.startsWith('data:')) {
          dataLines.push(buffer.slice(5).trimStart());
        }
      }
      flushEvent();
      break;
    }
  }

  return events;
}

async function parseHttpResponse(
  resp: Response,
  requestId?: number,
  expectJson = true,
): Promise<JsonRpcResponse | undefined> {
  if (!expectJson || resp.status === 202 || resp.status === 204) {
    return undefined;
  }

  const contentType = resp.headers.get('content-type') ?? '';
  if (contentType.includes('text/event-stream')) {
    const events = await readSseEvents(resp);
    for (const event of events) {
      try {
        const payload = JSON.parse(event.data) as JsonRpcResponse;
        if (requestId === undefined || payload.id === requestId) {
          return payload;
        }
      } catch {
        // Ignore non-JSON SSE events on request streams.
      }
    }
    throw new Error('MCP SSE stream ended without a matching JSON-RPC response');
  }

  const body = await resp.text();
  if (!body.trim()) {
    return undefined;
  }
  return JSON.parse(body) as JsonRpcResponse;
}

async function discoverLegacySseEndpoint(baseUrl: string, authToken?: string): Promise<string> {
  const url = validateMcpUrl(baseUrl).href;
  const headers: Record<string, string> = { Accept: 'text/event-stream' };
  if (authToken) {
    headers.Authorization = `Bearer ${authToken}`;
  }

  const resp = await fetch(url, { method: 'GET', headers });
  if (!resp.ok) {
    throw new McpHttpStatusError(resp.status, resp.statusText);
  }

  const events = await readSseEvents(resp);
  const endpointEvent = events.find((event) => event.event === 'endpoint' && event.data.trim());
  if (!endpointEvent) {
    throw new Error('Legacy MCP SSE endpoint discovery failed: missing endpoint event');
  }

  return new URL(endpointEvent.data.trim(), url).href;
}

async function sseRequest(
  baseUrl: string,
  request: JsonRpcRequest | Record<string, unknown>,
  authToken?: string,
  options?: { expectJson?: boolean; sessionId?: string },
): Promise<HttpRequestResult> {
  const url = resolveSseMessageUrl(baseUrl);
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 30_000);
  try {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      Accept: 'application/json, text/event-stream',
    };
    if (authToken) {
      headers['Authorization'] = `Bearer ${authToken}`;
    }
    if (options?.sessionId) {
      headers['Mcp-Session-Id'] = options.sessionId;
    }
    const resp = await fetch(url, {
      method: 'POST',
      headers,
      body: JSON.stringify(request),
      signal: controller.signal,
    });
    if (!resp.ok) {
      throw new McpHttpStatusError(resp.status, resp.statusText);
    }
    return {
      endpointUrl: url,
      sessionId: resp.headers.get('Mcp-Session-Id') ?? options?.sessionId,
      response: await parseHttpResponse(
        resp,
        'id' in request && typeof request.id === 'number' ? request.id : undefined,
        options?.expectJson ?? true,
      ),
    };
  } finally {
    clearTimeout(timeout);
  }
}

// ═══════════════════════════════════════════════════════════════════════════
// Stdio Transport (via Rust backend)
// ═══════════════════════════════════════════════════════════════════════════

async function stdioRequest(runtimeId: string, method: string, params?: Record<string, unknown>): Promise<unknown> {
  return api.mcpSendRequest(runtimeId, method, params ?? {});
}

// ═══════════════════════════════════════════════════════════════════════════
// MCP Client
// ═══════════════════════════════════════════════════════════════════════════

function extractResult(response: JsonRpcResponse): unknown {
  if (response.error) {
    throw new Error(`MCP error ${response.error.code}: ${response.error.message}`);
  }
  if (!('result' in response)) {
    throw new Error('MCP response missing result');
  }
  return response.result;
}

export async function connectMcpServer(state: McpServerState): Promise<McpServerState> {
  const { config } = state;
  let runtimeId: string | undefined;

  try {
    if (config.transport === 'stdio') {
      // Spawn process via Rust backend
      runtimeId = await api.mcpSpawnServer(
        config.command ?? '',
        config.args ?? [],
        config.env ?? {},
      );

      // Initialize and capture capabilities
      const initResult = await stdioRequest(runtimeId, 'initialize', {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'OxideTerm', version: '1.0.0' },
      }) as { capabilities?: McpServerCapabilities } | undefined;
      const capabilities = initResult?.capabilities;

      // Notify initialized
      await stdioRequest(runtimeId, 'notifications/initialized');

      // List tools only if the server advertises the capability.
      let tools: McpToolSchema[] = [];
      if (capabilities?.tools) {
        const toolsResult = await stdioRequest(runtimeId, 'tools/list') as { tools?: McpToolSchema[] } | undefined;
        tools = toolsResult?.tools ?? [];
      }

      // List resources if server advertises the capability
      let resources: McpResource[] = [];
      if (capabilities?.resources) {
        const resResult = await stdioRequest(runtimeId, 'resources/list') as { resources?: McpResource[] } | undefined;
        resources = resResult?.resources ?? [];
      }

      return { ...state, status: 'connected', runtimeId, capabilities, tools, resources, error: undefined };

    } else {
      // SSE transport
      let endpointUrl = config.url ?? '';
      let sessionId: string | undefined;
      const token = await getMcpAuthToken(config);

      // Initialize
      const initReq = makeRequest('initialize', {
        protocolVersion: '2024-11-05',
        capabilities: {},
        clientInfo: { name: 'OxideTerm', version: '1.0.0' },
      });
      let initTransport: HttpRequestResult;
      try {
        initTransport = await sseRequest(endpointUrl, initReq, token);
      } catch (error) {
        if (!(error instanceof McpHttpStatusError) || error.status < 400 || error.status >= 500) {
          throw error;
        }
        endpointUrl = await discoverLegacySseEndpoint(endpointUrl, token);
        initTransport = await sseRequest(endpointUrl, initReq, token);
      }
      endpointUrl = initTransport.endpointUrl;
      sessionId = initTransport.sessionId;
      const initResult = extractResult(initTransport.response as JsonRpcResponse) as { capabilities?: McpServerCapabilities } | undefined;
      const capabilities = initResult?.capabilities;

      // Notify initialized (notification — no id)
      const notifyMsg = makeNotification('notifications/initialized');
      const notifyTransport = await sseRequest(endpointUrl, notifyMsg, token, { expectJson: false, sessionId });
      sessionId = notifyTransport.sessionId ?? sessionId;

      // List tools (only if server advertises tools capability)
      let tools: McpToolSchema[] = [];
      if (capabilities?.tools) {
        const listReq = makeRequest('tools/list');
        const listTransport = await sseRequest(endpointUrl, listReq, token, { sessionId });
        sessionId = listTransport.sessionId ?? sessionId;
        const listResult = extractResult(listTransport.response as JsonRpcResponse) as { tools?: McpToolSchema[] } | undefined;
        tools = listResult?.tools ?? [];
      }

      // List resources (only if server advertises resources capability)
      let resources: McpResource[] = [];
      if (capabilities?.resources) {
        const resReq = makeRequest('resources/list');
        const resTransport = await sseRequest(endpointUrl, resReq, token, { sessionId });
        sessionId = resTransport.sessionId ?? sessionId;
        const resResult = extractResult(resTransport.response as JsonRpcResponse) as { resources?: McpResource[] } | undefined;
        resources = resResult?.resources ?? [];
      }

      return { ...state, status: 'connected', endpointUrl, sessionId, capabilities, tools, resources, error: undefined };
    }
  } catch (e) {
    if (config.transport === 'stdio' && runtimeId) {
      await api.mcpCloseServer(runtimeId).catch(() => {});
    }
    const message = e instanceof Error ? e.message : String(e);
    console.error(`[MCP] Failed to connect to ${config.name}:`, message);
    return { ...state, status: 'error', error: message, tools: [], resources: [] };
  }
}

export async function disconnectMcpServer(state: McpServerState): Promise<McpServerState> {
  try {
    if (state.runtimeId) {
      await api.mcpCloseServer(state.runtimeId);
    }
  } catch (e) {
    console.warn(`[MCP] Error disconnecting ${state.config.name}:`, e);
  }
  return { ...state, status: 'disconnected', runtimeId: undefined, tools: [], resources: [], error: undefined };
}

export async function callMcpTool(
  state: McpServerState,
  toolName: string,
  args: Record<string, unknown>,
): Promise<McpCallToolResult> {
  const params = { name: toolName, arguments: args };

  if (state.config.transport === 'stdio' && state.runtimeId) {
    const result = await stdioRequest(state.runtimeId, 'tools/call', params);
    return result as McpCallToolResult;
  } else if (state.config.transport === 'sse' && (state.endpointUrl || state.config.url)) {
    const token = await getMcpAuthToken(state.config);
    const req = makeRequest('tools/call', params);
    const resp = await sseRequest(state.endpointUrl ?? state.config.url ?? '', req, token, { sessionId: state.sessionId });
    return extractResult(resp.response as JsonRpcResponse) as McpCallToolResult;
  }

  throw new Error(`MCP server ${state.config.name} is not connected`);
}

export async function readMcpResource(
  state: McpServerState,
  uri: string,
): Promise<McpResourceContent> {
  const params = { uri };

  if (state.config.transport === 'stdio' && state.runtimeId) {
    const result = await stdioRequest(state.runtimeId, 'resources/read', params);
    const contents = (result as { contents?: McpResourceContent[] })?.contents;
    if (!contents?.length) throw new Error(`Empty resource response for ${uri}`);
    return contents[0];
  } else if (state.config.transport === 'sse' && (state.endpointUrl || state.config.url)) {
    const token = await getMcpAuthToken(state.config);
    const req = makeRequest('resources/read', params);
    const resp = await sseRequest(state.endpointUrl ?? state.config.url ?? '', req, token, { sessionId: state.sessionId });
    const result = extractResult(resp.response as JsonRpcResponse) as { contents?: McpResourceContent[] } | undefined;
    const contents = result?.contents;
    if (!contents?.length) throw new Error(`Empty resource response for ${uri}`);
    return contents[0];
  }

  throw new Error(`MCP server ${state.config.name} is not connected`);
}

export async function refreshMcpTools(state: McpServerState): Promise<McpToolSchema[]> {
  if (state.status !== 'connected') return [];
  if (!state.capabilities?.tools) return [];

  if (state.config.transport === 'stdio' && state.runtimeId) {
    const result = await stdioRequest(state.runtimeId, 'tools/list') as { tools?: McpToolSchema[] } | undefined;
    return result?.tools ?? [];
  } else if (state.config.transport === 'sse' && (state.endpointUrl || state.config.url)) {
    const token = await getMcpAuthToken(state.config);
    const req = makeRequest('tools/list');
    const resp = await sseRequest(state.endpointUrl ?? state.config.url ?? '', req, token, { sessionId: state.sessionId });
    const result = extractResult(resp.response as JsonRpcResponse) as { tools?: McpToolSchema[] } | undefined;
    return result?.tools ?? [];
  }

  return [];
}
