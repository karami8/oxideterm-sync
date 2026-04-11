// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

/**
 * React Error Boundary Component
 * 
 * 捕获子组件树中的 JavaScript 错误，防止整个应用崩溃。
 * 显示友好的错误界面并提供恢复选项。
 */

import { Component, ErrorInfo, ReactNode } from 'react';
import { AlertTriangle, RefreshCw, Bug, Copy, Check } from 'lucide-react';
import { Button } from './ui/button';
import i18n from '../i18n';

interface Props {
  children: ReactNode;
  /** 自定义 fallback UI */
  fallback?: ReactNode;
  /** 错误发生时的回调 */
  onError?: (error: Error, errorInfo: ErrorInfo) => void;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
  copied: boolean;
}

export class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
    errorInfo: null,
    copied: false,
  };

  public static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('[ErrorBoundary] Uncaught error:', error);
    console.error('[ErrorBoundary] Component stack:', errorInfo.componentStack);
    
    this.setState({ errorInfo });
    this.props.onError?.(error, errorInfo);
  }

  private handleReload = () => {
    window.location.reload();
  };

  private handleReset = () => {
    this.setState({ hasError: false, error: null, errorInfo: null });
  };

  private handleCopyError = async () => {
    const { error, errorInfo } = this.state;
    const errorText = [
      `Error: ${error?.message}`,
      `\nStack:\n${error?.stack}`,
      `\nComponent Stack:\n${errorInfo?.componentStack}`,
    ].join('\n');

    try {
      await navigator.clipboard.writeText(errorText);
      this.setState({ copied: true });
      setTimeout(() => this.setState({ copied: false }), 2000);
    } catch (err) {
      console.error('Failed to copy error:', err);
    }
  };

  public render() {
    if (this.state.hasError) {
      // If a custom fallback is provided, use it
      if (this.props.fallback) {
        return this.props.fallback;
      }

      const { error, errorInfo, copied } = this.state;

      return (
        <div className="min-h-screen bg-theme-bg flex items-center justify-center p-8">
          <div className="max-w-2xl w-full bg-theme-bg-panel border border-theme-border rounded-lg shadow-xl overflow-hidden">
            {/* Header */}
            <div className="bg-red-500/10 border-b border-red-500/20 px-6 py-4 flex items-center gap-3">
              <AlertTriangle className="h-6 w-6 text-red-500" />
              <div>
                <h1 className="text-lg font-semibold text-red-400">{i18n.t('error_boundary.title')}</h1>
                <p className="text-sm text-theme-text-muted">{i18n.t('error_boundary.description')}</p>
              </div>
            </div>

            {/* Error Details */}
            <div className="p-6 space-y-4">
              {/* Error Message */}
              <div className="bg-theme-bg rounded-lg p-4 border border-theme-border">
                <div className="flex items-start gap-2">
                  <Bug className="h-5 w-5 text-red-400 mt-0.5 flex-shrink-0" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm font-medium text-red-400 break-words">
                      {error?.message || i18n.t('error_boundary.unknown_error')}
                    </p>
                    {error?.stack && (
                      <pre className="mt-2 text-xs text-theme-text-muted overflow-x-auto whitespace-pre-wrap max-h-32 overflow-y-auto">
                        {error.stack.split('\n').slice(1, 5).join('\n')}
                      </pre>
                    )}
                  </div>
                </div>
              </div>

              {/* Component Stack (collapsible) */}
              {errorInfo?.componentStack && (
                <details className="bg-theme-bg rounded-lg border border-theme-border">
                  <summary className="px-4 py-2 text-sm text-theme-text-muted cursor-pointer hover:bg-theme-bg-hover">
                    {i18n.t('error_boundary.component_stack')}
                  </summary>
                  <pre className="px-4 pb-4 text-xs text-theme-text-muted overflow-x-auto whitespace-pre-wrap max-h-48 overflow-y-auto">
                    {errorInfo.componentStack}
                  </pre>
                </details>
              )}

              {/* Actions */}
              <div className="flex flex-wrap gap-3 pt-2">
                <Button
                  onClick={this.handleReset}
                  className="flex items-center gap-2"
                >
                  <RefreshCw className="h-4 w-4" />
                  {i18n.t('error_boundary.attempt_recovery')}
                </Button>
                <Button
                  variant="outline"
                  onClick={this.handleReload}
                  className="flex items-center gap-2"
                >
                  <RefreshCw className="h-4 w-4" />
                  {i18n.t('error_boundary.reload_page')}
                </Button>
                <Button
                  variant="ghost"
                  onClick={this.handleCopyError}
                  className="flex items-center gap-2 ml-auto"
                >
                  {copied ? (
                    <>
                      <Check className="h-4 w-4 text-green-500" />
                      {i18n.t('error_boundary.copied')}
                    </>
                  ) : (
                    <>
                      <Copy className="h-4 w-4" />
                      {i18n.t('error_boundary.copy_error')}
                    </>
                  )}
                </Button>
              </div>

              {/* Help Text */}
              <p className="text-xs text-theme-text-muted">
                {i18n.t('error_boundary.help_text')}{' '}
                <a 
                  href="https://github.com/karami8/oxideterm-sync/issues" 
                  target="_blank" 
                  rel="noopener noreferrer"
                  className="text-theme-accent hover:underline"
                >
                  {i18n.t('error_boundary.github_issues')}
                </a>
                {i18n.t('error_boundary.help_text_suffix')}
              </p>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

export default ErrorBoundary;
