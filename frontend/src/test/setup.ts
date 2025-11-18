import { afterEach, vi } from 'vitest'
import { cleanup } from '@testing-library/react'
import '@testing-library/jest-dom/vitest'

// Mock localStorage/sessionStorage for happy-dom (vitest 4 compatibility)
class MockStorage implements Storage {
  private store: Record<string, string> = {}

  get length(): number {
    return Object.keys(this.store).length
  }

  clear(): void {
    this.store = {}
  }

  getItem(key: string): string | null {
    return this.store[key] ?? null
  }

  key(index: number): string | null {
    const keys = Object.keys(this.store)
    return keys[index] ?? null
  }

  removeItem(key: string): void {
    delete this.store[key]
  }

  setItem(key: string, value: string): void {
    this.store[key] = value
  }
}

;(globalThis as unknown as { localStorage: Storage }).localStorage = new MockStorage()
;(globalThis as unknown as { sessionStorage: Storage }).sessionStorage = new MockStorage()

// Mock WebSocket for happy-dom (doesn't have WebSocket built-in)
class MockWebSocket {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSING = 2
  static readonly CLOSED = 3

  readonly CONNECTING = 0
  readonly OPEN = 1
  readonly CLOSING = 2
  readonly CLOSED = 3

  readyState = MockWebSocket.CONNECTING

  constructor(public url: string) {}

  send = vi.fn()
  close = vi.fn()
  addEventListener = vi.fn()
  removeEventListener = vi.fn()
}

;(globalThis as unknown as { WebSocket: typeof MockWebSocket }).WebSocket = MockWebSocket

// Mock Prism globally for all tests
// Use globalThis which works in both Node and browser environments
interface GlobalWithPrism {
  Prism: {
    highlightElement: ReturnType<typeof vi.fn>
    highlightAll: ReturnType<typeof vi.fn>
    languages: Record<string, unknown>
  }
}

;(globalThis as unknown as GlobalWithPrism).Prism = {
  highlightElement: vi.fn(),
  highlightAll: vi.fn(),
  languages: {},
}

// Mock Mermaid globally for all tests
vi.mock('mermaid', () => ({
  default: {
    initialize: vi.fn(),
    render: vi.fn().mockResolvedValue({ svg: '<svg>test diagram</svg>' }),
    run: vi.fn(),
  },
}))

// Cleanup after each test
afterEach(() => {
  cleanup()
  localStorage.clear()
  sessionStorage.clear()
})
