import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import { playwright } from '@vitest/browser-playwright'

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    setupFiles: './src/test/setup.ts',
    // Include only visual tests
    include: ['**/*.visual.test.{ts,tsx}'],
    // Browser mode configuration for visual/component tests
    browser: {
      enabled: true,
      provider: playwright({
        launchOptions: {
          headless: true,
        },
      }),
      instances: [
        { browser: 'chromium' },
      ],
      screenshotFailures: true,
      viewport: {
        width: 1280,
        height: 720,
      },
    },
  },
})
