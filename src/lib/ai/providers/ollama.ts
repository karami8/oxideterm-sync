/**
 * Ollama Provider Adapter
 *
 * Supports local Ollama instances.
 * Uses the OpenAI-compatible `/v1/chat/completions` endpoint (Ollama >= 0.1.14).
 */

import type { AiStreamProvider, AiRequestConfig, ChatMessage, AiStreamEvent } from '../providers';
import { getModelContextWindow } from '../tokenUtils';

/** Timeout for individual /api/show calls (ms) */
const OLLAMA_SHOW_TIMEOUT = 2000;
/** Max "wild" models to query via API (those not in the static lookup table) */
const MAX_WILD_MODELS_QUERY = 20;

export const ollamaProvider: AiStreamProvider = {
  type: 'ollama',
  displayName: 'Ollama (Local)',

  async *streamCompletion(
    config: AiRequestConfig,
    messages: ChatMessage[],
    signal: AbortSignal
  ): AsyncGenerator<AiStreamEvent> {
    const cleanBaseUrl = config.baseUrl.replace(/\/+$/, '');
    // Use Ollama's OpenAI-compatible endpoint
    const url = `${cleanBaseUrl}/v1/chat/completions`;

    let response: Response;
    try {
      response = await fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          // Ollama doesn't require auth but we send it if configured
          ...(config.apiKey ? { 'Authorization': `Bearer ${config.apiKey}` } : {}),
        },
        body: JSON.stringify({
          model: config.model,
          messages,
          stream: true,
          ...(config.maxResponseTokens ? { max_tokens: config.maxResponseTokens } : {}),
        }),
        signal,
      });
    } catch (e) {
      yield { type: 'error', message: 'Cannot connect to Ollama. Make sure Ollama is running (ollama serve).' };
      return;
    }

    if (!response.ok) {
      const errorText = await response.text();
      let errorMessage = `Ollama error: ${response.status}`;

      // Special handling for connection refused (Ollama not running)
      if (response.status === 0 || errorText.includes('ECONNREFUSED')) {
        errorMessage = 'Cannot connect to Ollama. Make sure Ollama is running (ollama serve).';
      } else {
        try {
          const errorJson = JSON.parse(errorText);
          errorMessage = errorJson.error?.message || errorJson.error || errorMessage;
        } catch {
          if (errorText) errorMessage = errorText.slice(0, 200);
        }
      }

      yield { type: 'error', message: errorMessage };
      return;
    }

    const reader = response.body?.getReader();
    if (!reader) {
      yield { type: 'error', message: 'No response body' };
      return;
    }

    const decoder = new TextDecoder();
    let buffer = '';

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        buffer += decoder.decode(value, { stream: true });
        const lines = buffer.split('\n');
        buffer = lines.pop() || '';

        for (const line of lines) {
          if (line.startsWith('data: ')) {
            const data = line.slice(6).trim();
            if (data === '[DONE]') {
              yield { type: 'done' };
              return;
            }

            try {
              const json = JSON.parse(data);
              // Handle DeepSeek-R1 style thinking in Ollama
              const delta = json.choices?.[0]?.delta;
              if (delta?.reasoning_content) {
                yield { type: 'thinking', content: delta.reasoning_content };
              }
              if (delta?.content) {
                yield { type: 'content', content: delta.content };
              }
            } catch {
              // Ignore parse errors
            }
          }
        }
      }
    } finally {
      reader.releaseLock();
    }

    yield { type: 'done' };
  },

  async fetchModels(config: { baseUrl: string; apiKey: string }): Promise<string[]> {
    const cleanBaseUrl = config.baseUrl.replace(/\/+$/, '');
    // Try Ollama native /api/tags first
    let resp: Response;
    try {
      resp = await fetch(`${cleanBaseUrl}/api/tags`, {
        headers: config.apiKey ? { 'Authorization': `Bearer ${config.apiKey}` } : {},
      });
    } catch (e) {
      throw new Error('Cannot connect to Ollama. Make sure Ollama is running (ollama serve).');
    }
    if (!resp.ok) throw new Error(`Failed to fetch models: ${resp.status}`);
    const data = await resp.json();
    if (!Array.isArray(data.models)) return [];
    return data.models
      .map((m: { name: string }) => m.name)
      .sort();
  },

  async fetchModelDetails(config: { baseUrl: string; apiKey: string }): Promise<Record<string, number>> {
    const cleanBaseUrl = config.baseUrl.replace(/\/+$/, '');
    // First get all model names
    let resp: Response;
    try {
      resp = await fetch(`${cleanBaseUrl}/api/tags`, {
        headers: config.apiKey ? { 'Authorization': `Bearer ${config.apiKey}` } : {},
      });
    } catch {
      return {};
    }
    if (!resp.ok) return {};
    const data = await resp.json();
    if (!Array.isArray(data.models)) return {};

    const result: Record<string, number> = {};

    // Separate models into "known" (matched by static lookup table) and "wild" (unknown)
    const wildModels: string[] = [];
    for (const m of data.models) {
      const staticCtx = getModelContextWindow(m.name);
      // If static lookup returns the default 8192 fallback, it means no match → wild model
      if (staticCtx !== 8192) {
        result[m.name] = staticCtx;
      } else {
        wildModels.push(m.name);
      }
    }

    // Only query wild models via API (parallel with timeout, capped)
    const toQuery = wildModels.slice(0, MAX_WILD_MODELS_QUERY);
    if (toQuery.length > 0) {
      const queryResults = await Promise.allSettled(
        toQuery.map(async (name) => {
          const controller = new AbortController();
          const timeout = setTimeout(() => controller.abort(), OLLAMA_SHOW_TIMEOUT);
          try {
            const showResp = await fetch(`${cleanBaseUrl}/api/show`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ name }),
              signal: controller.signal,
            });
            if (showResp.ok) {
              const showData = await showResp.json();
              const ctx = showData.model_info?.['general.context_length']
                ?? showData.model_info?.context_length
                ?? showData.parameters?.num_ctx;
              if (typeof ctx === 'number' && ctx > 0) {
                return { name, ctx };
              }
            }
            return null;
          } finally {
            clearTimeout(timeout);
          }
        })
      );

      for (const r of queryResults) {
        if (r.status === 'fulfilled' && r.value) {
          result[r.value.name] = r.value.ctx;
        }
      }
    }

    return result;
  },
};
