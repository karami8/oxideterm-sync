import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Info } from 'lucide-react';
import { useAiChatStore } from '../../store/aiChatStore';
import { useSettingsStore } from '../../store/settingsStore';
import { estimateTokens, getModelContextWindow } from '../../lib/ai/tokenUtils';
import { DEFAULT_SYSTEM_PROMPT } from '../../lib/ai/constants';

interface TokenBreakdown {
  system: number;
  history: number;
  context: number;
  total: number;
}

// ═══════════════════════════════════════════════════════════════════════════
// Context Window Indicator Component
// ═══════════════════════════════════════════════════════════════════════════

interface ContextIndicatorProps {
  pendingInput?: string;
}

export function ContextIndicator({ pendingInput = '' }: ContextIndicatorProps) {
  const { t } = useTranslation();
  const aiSettings = useSettingsStore((s) => s.settings.ai);
  const { activeConversationId, conversations } = useAiChatStore();
  
  // Get active conversation
  const conversation = conversations.find((c) => c.id === activeConversationId);

  // Resolve active model name
  const activeModel = aiSettings.activeModel
    || aiSettings.providers?.find(p => p.id === aiSettings.activeProviderId)?.defaultModel
    || aiSettings.model
    || '';
  
  // Calculate token breakdown
  const breakdown = useMemo<TokenBreakdown>(() => {
    // System prompt tokens (custom or default)
    const effectivePrompt = aiSettings.customSystemPrompt?.trim() || DEFAULT_SYSTEM_PROMPT;
    const systemTokens = estimateTokens(effectivePrompt);
    
    // History tokens (all messages — the API layer trims dynamically)
    let historyTokens = 0;
    if (conversation) {
      for (const msg of conversation.messages) {
        if (msg.role === 'user' || msg.role === 'assistant') {
          historyTokens += estimateTokens(msg.content);
        }
      }
    }
    
    // Pending input + context
    const contextTokens = estimateTokens(pendingInput);
    
    return {
      system: systemTokens,
      history: historyTokens,
      context: contextTokens,
      total: systemTokens + historyTokens + contextTokens,
    };
  }, [conversation?.messages, pendingInput, aiSettings.customSystemPrompt]);
  
  // Context window from cached provider data or fallback pattern matching
  const maxTokens = useMemo(() => {
    return getModelContextWindow(activeModel, aiSettings.modelContextWindows, aiSettings.activeProviderId ?? undefined);
  }, [activeModel, aiSettings.modelContextWindows, aiSettings.activeProviderId]);
  
  const percentage = Math.min((breakdown.total / maxTokens) * 100, 100);
  const isWarning = percentage > 70;
  const isDanger = percentage > 90;
  
  // Color based on usage
  const barColor = isDanger 
    ? 'bg-red-500' 
    : isWarning 
      ? 'bg-amber-500' 
      : 'bg-theme-accent';
  
  const textColor = isDanger
    ? 'text-red-500'
    : isWarning
      ? 'text-amber-500'
      : 'text-theme-text-muted';
  
  // Format number with K suffix
  const formatTokens = (n: number) => {
    if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
    return n.toString();
  };
  
  // Build tooltip text
  const tooltipText = [
    `${t('ai.context.system')}: ${formatTokens(breakdown.system)}`,
    `${t('ai.context.history')}: ${formatTokens(breakdown.history)}`,
    `${t('ai.context.pending')}: ${formatTokens(breakdown.context)}`,
    `${t('ai.context.total')}: ${formatTokens(breakdown.total)} / ${formatTokens(maxTokens)}`,
    isDanger ? `⚠️ ${t('ai.context.warning_limit')}` : '',
  ].filter(Boolean).join('\n');
  
  return (
    <div 
      className="flex items-center gap-1.5 sm:gap-2 cursor-help group shrink-0"
      title={tooltipText}
    >
      <Info className={`w-3 h-3 shrink-0 ${textColor} opacity-50 group-hover:opacity-100`} />
      
      {/* Mini progress bar */}
      <div className="w-10 sm:w-16 h-1 bg-theme-border/20 rounded-full overflow-hidden">
        <div 
          className={`h-full ${barColor}`}
          style={{ width: `${percentage}%` }}
        />
      </div>
      
      {/* Token count - always visible but compact */}
      <span className={`text-[9px] font-mono ${textColor} opacity-60 whitespace-nowrap`}>
        {formatTokens(breakdown.total)}
      </span>
    </div>
  );
}
