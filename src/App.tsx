import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/tauri';
import { listen } from '@tauri-apps/api/event';

type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'error';

interface LogEntry {
  timestamp: string;
  message: string;
  type: 'info' | 'error' | 'success';
}

function App() {
  const [status, setStatus] = useState<ConnectionStatus>('disconnected');
  const [token, setToken] = useState('');
  const [savedToken, setSavedToken] = useState('');
  const [models, setModels] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [ollamaStatus, setOllamaStatus] = useState<'unknown' | 'running' | 'stopped'>('unknown');

  const addLog = useCallback((message: string, type: LogEntry['type'] = 'info') => {
    const entry: LogEntry = {
      timestamp: new Date().toLocaleTimeString(),
      message,
      type,
    };
    setLogs((prev) => [...prev.slice(-99), entry]);
  }, []);

  useEffect(() => {
    // Load saved token on mount
    invoke<string | null>('get_saved_token')
      .then((token) => {
        if (token) {
          setSavedToken(token);
          setToken(token);
          addLog('Loaded saved token');
        }
      })
      .catch((err) => {
        console.error('Failed to load token:', err);
      });

    // Check Ollama status
    invoke<boolean>('check_ollama')
      .then((running) => {
        setOllamaStatus(running ? 'running' : 'stopped');
        addLog(running ? 'Ollama is running' : 'Ollama is not running', running ? 'success' : 'error');
      })
      .catch(() => {
        setOllamaStatus('stopped');
        addLog('Could not connect to Ollama', 'error');
      });

    // Listen for events from Rust backend
    const unlistenStatus = listen<{ status: ConnectionStatus; error?: string }>('connection-status', (event) => {
      setStatus(event.payload.status);
      if (event.payload.error) {
        setError(event.payload.error);
        addLog(event.payload.error, 'error');
      } else if (event.payload.status === 'connected') {
        addLog('Connected to BottleCapAI', 'success');
      } else if (event.payload.status === 'disconnected') {
        addLog('Disconnected from BottleCapAI', 'info');
      }
    });

    const unlistenModels = listen<string[]>('models-updated', (event) => {
      setModels(event.payload);
      addLog(`Found ${event.payload.length} models: ${event.payload.join(', ')}`, 'info');
    });

    const unlistenLog = listen<{ message: string; type: LogEntry['type'] }>('log-message', (event) => {
      addLog(event.payload.message, event.payload.type);
    });

    return () => {
      unlistenStatus.then((fn) => fn());
      unlistenModels.then((fn) => fn());
      unlistenLog.then((fn) => fn());
    };
  }, [addLog]);

  const handleConnect = async () => {
    if (!token.startsWith('bc_runner_')) {
      setError('Invalid token format. Token should start with bc_runner_');
      return;
    }

    setStatus('connecting');
    setError(null);
    addLog('Connecting to BottleCapAI...');

    try {
      await invoke('connect_to_partykit', { token });
      // Save token for next time
      await invoke('save_token', { token });
      setSavedToken(token);
    } catch (err: unknown) {
      setStatus('error');
      const errorMessage = err instanceof Error ? err.message : String(err);
      setError(errorMessage);
      addLog(`Connection failed: ${errorMessage}`, 'error');
    }
  };

  const handleDisconnect = async () => {
    addLog('Disconnecting...');
    try {
      await invoke('disconnect');
      setStatus('disconnected');
      setModels([]);
    } catch (err) {
      console.error('Disconnect error:', err);
    }
  };

  const clearToken = async () => {
    await invoke('clear_token');
    setSavedToken('');
    setToken('');
    addLog('Token cleared');
  };

  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 p-6">
      <div className="max-w-xl mx-auto space-y-5">
        {/* Header */}
        <div className="text-center pb-2">
          <h1 className="text-2xl font-bold text-slate-800">BottleCapAI Runner</h1>
          <p className="text-slate-500 text-sm">Connect your local LLMs to the cloud</p>
        </div>

        {/* Ollama Status */}
        <div className="bg-white rounded-xl p-4 shadow-sm border border-slate-200">
          <div className="flex items-center justify-between">
            <span className="text-sm font-medium text-slate-700">Ollama Status</span>
            <span
              className={`text-xs px-2 py-1 rounded-full ${
                ollamaStatus === 'running'
                  ? 'bg-green-100 text-green-700'
                  : ollamaStatus === 'stopped'
                  ? 'bg-red-100 text-red-700'
                  : 'bg-slate-100 text-slate-500'
              }`}
            >
              {ollamaStatus === 'running' ? 'Running' : ollamaStatus === 'stopped' ? 'Not Running' : 'Checking...'}
            </span>
          </div>
          {ollamaStatus === 'stopped' && (
            <div className="mt-3 p-3 bg-amber-50 border border-amber-200 rounded-lg">
              <p className="text-sm font-medium text-amber-800">Ollama not detected</p>
              <p className="text-xs text-amber-700 mt-1">
                Ollama is required to run local LLMs. Follow these steps:
              </p>
              <ol className="text-xs text-amber-700 mt-2 ml-4 list-decimal space-y-1">
                <li>
                  <a href="https://ollama.com/download" target="_blank" rel="noopener noreferrer" className="text-amber-900 font-medium hover:underline">
                    Download and install Ollama
                  </a>
                </li>
                <li>Open Ollama (it runs in the background)</li>
                <li>
                  Open Terminal and run: <code className="bg-amber-100 px-1 rounded">ollama pull llama3.2</code>
                </li>
              </ol>
              <button
                onClick={() => {
                  invoke<boolean>('check_ollama').then((running) => {
                    setOllamaStatus(running ? 'running' : 'stopped');
                    addLog(running ? 'Ollama is now running!' : 'Ollama still not detected', running ? 'success' : 'error');
                  });
                }}
                className="mt-3 text-xs text-amber-800 font-medium hover:text-amber-900"
              >
                Check again →
              </button>
            </div>
          )}
          {ollamaStatus === 'running' && models.length === 0 && status !== 'connected' && (
            <div className="mt-3 p-3 bg-blue-50 border border-blue-200 rounded-lg">
              <p className="text-sm font-medium text-blue-800">No models found</p>
              <p className="text-xs text-blue-700 mt-1">
                Download a model to get started. Open Terminal and run:
              </p>
              <code className="block text-xs bg-blue-100 text-blue-800 px-2 py-1 rounded mt-2">
                ollama pull llama3.2
              </code>
              <p className="text-xs text-blue-600 mt-2">
                Other popular models: <code className="bg-blue-100 px-1 rounded">mistral</code>, <code className="bg-blue-100 px-1 rounded">codellama</code>, <code className="bg-blue-100 px-1 rounded">deepseek-r1</code>
              </p>
            </div>
          )}
        </div>

        {/* Connection Card */}
        <div className="bg-white rounded-xl p-5 shadow-sm border border-slate-200">
          <div className="flex items-center justify-between mb-4">
            <h2 className="font-semibold text-slate-700">Connection</h2>
            <StatusBadge status={status} />
          </div>

          {status === 'disconnected' || status === 'error' || status === 'connecting' ? (
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-slate-600 mb-1">Runner Token</label>
                <input
                  type="password"
                  value={token}
                  onChange={(e) => setToken(e.target.value)}
                  placeholder="bc_runner_..."
                  className="w-full px-3 py-2 border border-slate-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 outline-none text-sm"
                />
                <p className="text-xs text-slate-500 mt-1">
                  Get your token from the{' '}
                  <a href="https://bottlecap.ai/dashboard/runners" target="_blank" rel="noopener noreferrer" className="text-blue-600 hover:underline">
                    BottleCapAI dashboard
                  </a>
                </p>
              </div>

              {savedToken && token !== savedToken && (
                <button onClick={() => setToken(savedToken)} className="text-xs text-blue-600 hover:underline">
                  Use saved token
                </button>
              )}

              <button
                onClick={handleConnect}
                disabled={!token || status === 'connecting' || ollamaStatus !== 'running'}
                className="w-full py-2.5 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors font-medium"
              >
                {status === 'connecting' ? 'Connecting...' : 'Connect'}
              </button>

              {savedToken && (
                <button onClick={clearToken} className="w-full text-xs text-slate-500 hover:text-slate-700">
                  Clear saved token
                </button>
              )}
            </div>
          ) : (
            <div className="space-y-4">
              <div className="p-3 bg-green-50 border border-green-200 rounded-lg">
                <p className="text-green-700 text-sm font-medium">Connected and ready</p>
                <p className="text-green-600 text-xs mt-1">Waiting for requests from BottleCapAI</p>
              </div>
              <button
                onClick={handleDisconnect}
                className="w-full py-2 bg-slate-100 text-slate-700 rounded-lg hover:bg-slate-200 transition-colors font-medium"
              >
                Disconnect
              </button>
            </div>
          )}

          {error && (
            <div className="mt-4 p-3 bg-red-50 border border-red-200 rounded-lg">
              <p className="text-red-700 text-sm">{error}</p>
            </div>
          )}
        </div>

        {/* Available Models */}
        {status === 'connected' && models.length > 0 && (
          <div className="bg-white rounded-xl p-5 shadow-sm border border-slate-200">
            <h2 className="font-semibold text-slate-700 mb-3">Available Models</h2>
            <div className="flex flex-wrap gap-2">
              {models.map((model) => (
                <span key={model} className="px-3 py-1 bg-slate-100 text-slate-700 rounded-full text-sm">
                  {model}
                </span>
              ))}
            </div>
          </div>
        )}

        {/* Activity Log */}
        <div className="bg-white rounded-xl p-5 shadow-sm border border-slate-200">
          <h2 className="font-semibold text-slate-700 mb-3">Activity</h2>
          <div className="h-40 overflow-y-auto bg-slate-50 rounded-lg p-3 font-mono text-xs space-y-1">
            {logs.length === 0 ? (
              <p className="text-slate-400">No activity yet</p>
            ) : (
              logs.map((log, i) => (
                <div
                  key={i}
                  className={`${
                    log.type === 'error' ? 'text-red-600' : log.type === 'success' ? 'text-green-600' : 'text-slate-600'
                  }`}
                >
                  <span className="text-slate-400">[{log.timestamp}]</span> {log.message}
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: ConnectionStatus }) {
  const styles = {
    disconnected: 'bg-slate-100 text-slate-600',
    connecting: 'bg-yellow-100 text-yellow-700',
    connected: 'bg-green-100 text-green-700',
    error: 'bg-red-100 text-red-700',
  };

  const labels = {
    disconnected: 'Disconnected',
    connecting: 'Connecting...',
    connected: 'Connected',
    error: 'Error',
  };

  return <span className={`px-3 py-1 rounded-full text-xs font-medium ${styles[status]}`}>{labels[status]}</span>;
}

export default App;
