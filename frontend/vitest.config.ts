import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    // Use jsdom for unit tests by default
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
    // Exclude visual tests and playwright e2e tests from unit test runs
    exclude: ['**/*.visual.test.{ts,tsx}', '**/*.spec.{ts,tsx}', '**/node_modules/**', '**/e2e/**'],
    // Use threads pool for better CI compatibility
    pool: 'threads',
    poolOptions: {
      threads: {
        singleThread: true,
      },
    },
  },
})
