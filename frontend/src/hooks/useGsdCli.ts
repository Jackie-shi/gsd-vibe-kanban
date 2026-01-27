import { useCallback, useEffect, useRef, useState } from 'react';

// Base64 encoding/decoding utilities
function encodeBase64(str: string): string {
  const bytes = new TextEncoder().encode(str);
  const binString = Array.from(bytes, (b) => String.fromCodePoint(b)).join('');
  return btoa(binString);
}

function decodeBase64(base64: string): string {
  const binString = atob(base64);
  const bytes = Uint8Array.from(binString, (c) => c.codePointAt(0)!);
  return new TextDecoder().decode(bytes);
}

// Message types from backend
interface GsdCliStarted {
  type: 'started';
  session_id: string;
}

interface GsdCliOutput {
  type: 'output';
  data: string; // base64 encoded
}

interface GsdCliCompleted {
  type: 'completed';
  exit_code: number | null;
}

interface GsdCliError {
  type: 'error';
  message: string;
}

interface GsdCliStageChanged {
  type: 'stage_changed';
  stage: string;
}

type GsdCliMessage =
  | GsdCliStarted
  | GsdCliOutput
  | GsdCliCompleted
  | GsdCliError
  | GsdCliStageChanged;

export interface GsdCliState {
  isConnected: boolean;
  isRunning: boolean;
  sessionId: string | null;
  currentStage: string | null;
  output: string;
  exitCode: number | null;
  error: string | null;
}

export interface UseGsdCliOptions {
  projectPath: string;
  skill?: string;
  cols?: number;
  rows?: number;
  onOutput?: (data: string) => void;
  onStageChange?: (stage: string) => void;
  onComplete?: (exitCode: number | null) => void;
  onError?: (error: string) => void;
}

export interface UseGsdCliReturn {
  state: GsdCliState;
  connect: () => void;
  disconnect: () => void;
  sendInput: (text: string) => void;
  sendLine: (text: string) => void;
  resize: (cols: number, rows: number) => void;
  abort: () => void;
}

export function useGsdCli(options: UseGsdCliOptions): UseGsdCliReturn {
  const {
    projectPath,
    skill = 'new-project',
    cols = 120,
    rows = 40,
    onOutput,
    onStageChange,
    onComplete,
    onError,
  } = options;

  const [state, setState] = useState<GsdCliState>({
    isConnected: false,
    isRunning: false,
    sessionId: null,
    currentStage: null,
    output: '',
    exitCode: null,
    error: null,
  });

  const wsRef = useRef<WebSocket | null>(null);
  const callbacksRef = useRef({ onOutput, onStageChange, onComplete, onError });

  // Keep callbacks ref up to date
  useEffect(() => {
    callbacksRef.current = { onOutput, onStageChange, onComplete, onError };
  }, [onOutput, onStageChange, onComplete, onError]);

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      return;
    }

    // Build WebSocket URL
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.host;
    const params = new URLSearchParams({
      project_path: projectPath,
      skill,
      cols: cols.toString(),
      rows: rows.toString(),
    });
    const wsUrl = `${protocol}//${host}/api/gsd/cli/ws?${params}`;

    console.log('[GSD CLI] Connecting to:', wsUrl);

    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      console.log('[GSD CLI] WebSocket connected');
      setState((prev) => ({ ...prev, isConnected: true, error: null }));
    };

    ws.onmessage = (event) => {
      try {
        const msg: GsdCliMessage = JSON.parse(event.data);
        console.log('[GSD CLI] Received:', msg.type);

        switch (msg.type) {
          case 'started':
            setState((prev) => ({
              ...prev,
              isRunning: true,
              sessionId: msg.session_id,
            }));
            break;

          case 'output': {
            const decoded = decodeBase64(msg.data);
            setState((prev) => ({
              ...prev,
              output: prev.output + decoded,
            }));
            callbacksRef.current.onOutput?.(decoded);
            break;
          }

          case 'stage_changed':
            setState((prev) => ({
              ...prev,
              currentStage: msg.stage,
            }));
            callbacksRef.current.onStageChange?.(msg.stage);
            break;

          case 'completed':
            setState((prev) => ({
              ...prev,
              isRunning: false,
              exitCode: msg.exit_code,
            }));
            callbacksRef.current.onComplete?.(msg.exit_code);
            break;

          case 'error':
            setState((prev) => ({
              ...prev,
              error: msg.message,
            }));
            callbacksRef.current.onError?.(msg.message);
            break;
        }
      } catch (err) {
        console.error('[GSD CLI] Failed to parse message:', err);
      }
    };

    ws.onerror = (event) => {
      console.error('[GSD CLI] WebSocket error:', event);
      setState((prev) => ({
        ...prev,
        error: 'WebSocket connection error',
      }));
    };

    ws.onclose = (event) => {
      console.log('[GSD CLI] WebSocket closed:', event.code, event.reason);
      setState((prev) => ({
        ...prev,
        isConnected: false,
        isRunning: false,
      }));
      wsRef.current = null;
    };
  }, [projectPath, skill, cols, rows]);

  const disconnect = useCallback(() => {
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    setState({
      isConnected: false,
      isRunning: false,
      sessionId: null,
      currentStage: null,
      output: '',
      exitCode: null,
      error: null,
    });
  }, []);

  const sendInput = useCallback((text: string) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      const encoded = encodeBase64(text);
      wsRef.current.send(JSON.stringify({ type: 'input', data: encoded }));
    }
  }, []);

  const sendLine = useCallback(
    (text: string) => {
      sendInput(text + '\n');
    },
    [sendInput]
  );

  const resize = useCallback((newCols: number, newRows: number) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(
        JSON.stringify({ type: 'resize', cols: newCols, rows: newRows })
      );
    }
  }, []);

  const abort = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify({ type: 'abort' }));
    }
  }, []);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, []);

  return {
    state,
    connect,
    disconnect,
    sendInput,
    sendLine,
    resize,
    abort,
  };
}
