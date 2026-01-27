import { useEffect, useRef, useCallback, useState } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { WebLinksAddon } from '@xterm/addon-web-links';
import '@xterm/xterm/css/xterm.css';

import { useGsdCli, GsdCliState } from '@/hooks/useGsdCli';
import { useTheme } from '@/components/ThemeProvider';
import { getTerminalTheme } from '@/utils/terminalTheme';
import { Button } from '@/components/ui/button';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertCircle, Play, Square, RefreshCw } from 'lucide-react';

export interface GsdCliTerminalProps {
  projectPath: string;
  skill?: string;
  autoStart?: boolean;
  onComplete?: (exitCode: number | null) => void;
  onError?: (error: string) => void;
}

export function GsdCliTerminal({
  projectPath,
  skill = 'new-project',
  autoStart = true,
  onComplete,
  onError,
}: GsdCliTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const resizeRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const { theme } = useTheme();
  const [hasStarted, setHasStarted] = useState(false);

  // Handle terminal output
  const handleOutput = useCallback((data: string) => {
    if (terminalRef.current) {
      terminalRef.current.write(data);
    }
  }, []);

  // Use the GSD CLI hook
  const {
    state,
    connect,
    disconnect,
    sendInput,
    abort,
    resize: resizePty,
  } = useGsdCli({
    projectPath,
    skill,
    cols: 120,
    rows: 40,
    onOutput: handleOutput,
    onComplete,
    onError,
  });

  // Initialize terminal
  useEffect(() => {
    if (!containerRef.current || terminalRef.current) return;

    const terminal = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: '"IBM Plex Mono", "Fira Code", monospace',
      theme: getTerminalTheme(),
      scrollback: 10000,
      convertEol: true,
    });

    const fitAddon = new FitAddon();
    const webLinksAddon = new WebLinksAddon();

    terminal.loadAddon(fitAddon);
    terminal.loadAddon(webLinksAddon);
    terminal.open(containerRef.current);

    fitAddon.fit();

    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    // Handle user input - send to PTY
    terminal.onData((data) => {
      sendInput(data);
    });

    // Auto-start if enabled
    if (autoStart && !hasStarted) {
      setHasStarted(true);
      connect();
    }

    return () => {
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [sendInput, connect, autoStart, hasStarted]);

  // Handle resize
  useEffect(() => {
    if (!resizeRef.current) return;

    const observer = new ResizeObserver(() => {
      if (fitAddonRef.current && terminalRef.current) {
        fitAddonRef.current.fit();
        resizePty(terminalRef.current.cols, terminalRef.current.rows);
      }
    });

    observer.observe(resizeRef.current);
    return () => observer.disconnect();
  }, [resizePty]);

  // Update theme
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.theme = getTerminalTheme();
    }
  }, [theme]);

  const handleStart = () => {
    setHasStarted(true);
    connect();
  };

  const handleStop = () => {
    abort();
    disconnect();
  };

  const handleRestart = () => {
    disconnect();
    if (terminalRef.current) {
      terminalRef.current.clear();
    }
    setTimeout(() => {
      connect();
    }, 100);
  };

  return (
    <div className="flex flex-col h-full">
      {/* Status Bar */}
      <div className="flex items-center justify-between px-3 py-2 bg-muted/50 border-b">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <div
              className={`w-2 h-2 rounded-full ${
                state.isRunning
                  ? 'bg-green-500 animate-pulse'
                  : state.isConnected
                    ? 'bg-yellow-500'
                    : 'bg-gray-400'
              }`}
            />
            <span className="text-sm font-medium">
              {state.isRunning
                ? 'Running'
                : state.isConnected
                  ? 'Connected'
                  : 'Disconnected'}
            </span>
          </div>

          {state.currentStage && (
            <span className="text-sm text-muted-foreground">
              Stage: {state.currentStage}
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          {!state.isConnected && !state.isRunning && (
            <Button size="sm" variant="outline" onClick={handleStart}>
              <Play className="h-4 w-4 mr-1" />
              Start
            </Button>
          )}

          {state.isRunning && (
            <Button size="sm" variant="outline" onClick={handleStop}>
              <Square className="h-4 w-4 mr-1" />
              Stop
            </Button>
          )}

          {state.exitCode !== null && (
            <Button size="sm" variant="outline" onClick={handleRestart}>
              <RefreshCw className="h-4 w-4 mr-1" />
              Restart
            </Button>
          )}
        </div>
      </div>

      {/* Error Display */}
      {state.error && (
        <Alert variant="destructive" className="m-2 mb-0">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>{state.error}</AlertDescription>
        </Alert>
      )}

      {/* Terminal */}
      <div ref={resizeRef} className="flex-1 min-h-0">
        <div ref={containerRef} className="w-full h-full p-2" />
      </div>

      {/* Completion Status */}
      {state.exitCode !== null && (
        <div
          className={`px-3 py-2 border-t ${
            state.exitCode === 0
              ? 'bg-green-500/10 text-green-700 dark:text-green-400'
              : 'bg-red-500/10 text-red-700 dark:text-red-400'
          }`}
        >
          <span className="text-sm font-medium">
            {state.exitCode === 0
              ? 'GSD completed successfully!'
              : `GSD exited with code ${state.exitCode}`}
          </span>
        </div>
      )}
    </div>
  );
}

// Status component to show GSD progress
export function GsdCliStatus({ state }: { state: GsdCliState }) {
  return (
    <div className="flex items-center gap-2 text-sm">
      <div
        className={`w-2 h-2 rounded-full ${
          state.isRunning
            ? 'bg-green-500 animate-pulse'
            : state.error
              ? 'bg-red-500'
              : 'bg-gray-400'
        }`}
      />
      <span>
        {state.isRunning
          ? `Running${state.currentStage ? `: ${state.currentStage}` : ''}`
          : state.error
            ? 'Error'
            : state.exitCode !== null
              ? state.exitCode === 0
                ? 'Completed'
                : `Failed (${state.exitCode})`
              : 'Ready'}
      </span>
    </div>
  );
}
