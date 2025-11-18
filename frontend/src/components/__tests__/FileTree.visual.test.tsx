import { describe, it, expect, afterEach } from 'vitest'
import { page } from 'vitest/browser'
import { render } from '@testing-library/react'
import { FileTree } from '../FileTree'
import type { FileTreeNode } from '../../utils/fileTree'
import '../../App.css'

describe('FileTree Visual Tests', () => {
  const sampleNodes: FileTreeNode[] = [
    {
      name: 'docs',
      path: 'docs',
      isFolder: true,
      children: [
        { name: 'README.md', path: 'docs/README.md', isFolder: false },
        { name: 'guide.md', path: 'docs/guide.md', isFolder: false }
      ]
    },
    {
      name: 'src',
      path: 'src',
      isFolder: true,
      children: [
        { name: 'index.ts', path: 'src/index.ts', isFolder: false }
      ]
    },
    { name: 'package.json', path: 'package.json', isFolder: false },
    { name: 'tsconfig.json', path: 'tsconfig.json', isFolder: false }
  ]

  afterEach(() => {
    document.documentElement.removeAttribute('data-theme')
    document.body.innerHTML = ''
  })

  describe('SVG Icons (not emoji)', () => {
    it('shows SVG folder and file icons in dark theme', async () => {
      document.documentElement.setAttribute('data-theme', 'dark')
      render(
        <FileTree
          nodes={sampleNodes}
          currentPath="docs/README.md"
          onFileSelect={() => {}}
          isExpanded={(path) => path === 'docs'}
          onToggleFolder={() => {}}
        />
      )

      await expect.element(page.getByText('docs')).toBeInTheDocument()
      await page.screenshot()
    })

    it('shows SVG folder and file icons in light theme', async () => {
      document.documentElement.setAttribute('data-theme', 'light')
      render(
        <FileTree
          nodes={sampleNodes}
          currentPath="package.json"
          onFileSelect={() => {}}
          isExpanded={(path) => path === 'docs' || path === 'src'}
          onToggleFolder={() => {}}
        />
      )

      await expect.element(page.getByText('src')).toBeInTheDocument()
      await page.screenshot()
    })
  })
})
