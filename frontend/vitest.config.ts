import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
    exclude: ['**/*.visual.test.{ts,tsx}', '**/*.spec.{ts,tsx}', '**/node_modules/**', '**/e2e/**'],
    // Add github-actions reporter for CI
    reporters: process.env.GITHUB_ACTIONS ? ['default', 'github-actions'] : ['default'],
  },
})
