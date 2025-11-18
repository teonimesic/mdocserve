import { test, expect } from '@playwright/test'
import { spawn, type ChildProcess } from 'child_process'
import { mkdtempSync, writeFileSync, rmSync, renameSync, unlinkSync, mkdirSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'

// E2E tests for core mdocserve functionality
let serverProcess: ChildProcess | null = null
let testDir: string
const SERVER_PORT = 3456
const SERVER_URL = `http://127.0.0.1:${SERVER_PORT}`

test.beforeAll(async () => {
  // Create temp directory for test files
  testDir = mkdtempSync(join(tmpdir(), 'mdocserve-e2e-'))

  // Create test markdown files
  writeFileSync(join(testDir, 'test.md'), '# Test Document\n\nThis is a test document.')
  writeFileSync(join(testDir, 'another.md'), '# Another Document\n\nAnother test.')

  // Create nested folder with file
  mkdirSync(join(testDir, 'folder1'))
  writeFileSync(join(testDir, 'folder1', 'nested.md'), '# Nested Document\n\nNested content.')

  // Create test image
  const testImagePath = join(testDir, 'test-image.png')
  // Create a minimal 1x1 PNG
  const pngData = Buffer.from([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
  ])
  writeFileSync(testImagePath, pngData)

  // Start the mdocserve server
  const mdocservePath = join(process.cwd(), '../target/release/mdocserve')
  serverProcess = spawn(mdocservePath, [testDir, '--port', SERVER_PORT.toString()], {
    stdio: 'pipe',
    cwd: join(process.cwd(), '..'), // Run from project root so frontend/dist is found
  })

  serverProcess.on('error', (err) => {
    throw new Error(`Failed to start server: ${err.message}`)
  })

  // Wait for server to start
  let attempts = 0
  const maxAttempts = 10 // 2 seconds total
  while (attempts < maxAttempts) {
    try {
      const response = await fetch(`${SERVER_URL}/__health`)
      if (response.ok) {
        break
      }
    } catch {
      // Server not ready yet
    }
    await new Promise((resolve) => setTimeout(resolve, 200))
    attempts++
  }

  if (attempts >= maxAttempts) {
    throw new Error('Server failed to start in time')
  }
})

test.afterAll(async () => {
  // Kill server
  if (serverProcess) {
    serverProcess.kill()
    serverProcess = null
  }

  // Cleanup test directory
  if (testDir) {
    rmSync(testDir, { recursive: true, force: true })
  }
})

test('should render markdown content correctly', async ({ page }) => {
  await page.goto(SERVER_URL)

  // Wait for the app to load and files to be fetched
  await page.waitForSelector('.file-list', { timeout: 10000 })

  // Wait for test.md to appear and click it
  const testFileLink = page.getByText('test.md')
  await testFileLink.waitFor({ timeout: 10000 })
  await testFileLink.click()

  // Wait for the markdown content to be rendered
  await page.waitForSelector('h1', { timeout: 10000 })

  const heading = page.getByRole('heading', { name: 'Test Document' })
  await expect(heading).toBeVisible()

  const paragraph = page.getByText('This is a test document.')
  await expect(paragraph).toBeVisible()
})

test('should render nested folder files', async ({ page }) => {
  await page.goto(SERVER_URL)

  // Wait for file list to load
  await page.waitForSelector('.file-list')

  // Find and click folder by its text (folder1)
  const folder = page.getByText('folder1', { exact: true })
  await folder.waitFor()
  await folder.click()

  // Wait for nested file to appear (folder expands)
  const nestedLink = page.getByText('nested.md')
  await nestedLink.waitFor()
  await nestedLink.click()

  // Wait for content to load
  await page.waitForSelector('h1')

  const heading = page.getByRole('heading', { name: 'Nested Document' })
  await expect(heading).toBeVisible()

  const content = page.getByText('Nested content.')
  await expect(content).toBeVisible()
})

test('should display all markdown files in sidebar', async ({ page }) => {
  await page.goto(SERVER_URL)

  await page.waitForSelector('.file-list')

  // Check for root level files
  const testFile = page.getByText('test.md')
  await expect(testFile).toBeVisible()

  const anotherFile = page.getByText('another.md')
  await expect(anotherFile).toBeVisible()

  // Expand folder to see nested file
  const folder = page.getByText('folder1', { exact: true })
  await folder.click()

  // Check for nested file
  const nestedFile = page.getByText('nested.md')
  await expect(nestedFile).toBeVisible()
})

test('should switch between files when clicking in sidebar', async ({ page }) => {
  await page.goto(SERVER_URL)

  await page.waitForSelector('.file-list')

  // Click first file
  const testFile = page.getByText('test.md')
  await testFile.click()

  await page.waitForSelector('h1')
  let heading = page.getByRole('heading', { name: 'Test Document' })
  await expect(heading).toBeVisible()

  // Click second file
  const anotherFile = page.getByText('another.md')
  await anotherFile.click()

  await page.waitForSelector('h1')
  heading = page.getByRole('heading', { name: 'Another Document' })
  await expect(heading).toBeVisible()
})

test('should collapse and uncollapse folders, and navigate to nested files', async ({ page }) => {
  await page.goto(SERVER_URL)

  await page.waitForSelector('.file-list')

  // Find the folder1 folder item
  const folder = page.getByText('folder1', { exact: true })
  await expect(folder).toBeVisible()

  // Folder should be expanded by default (or collapsed - we'll test both states)
  // Try to find the nested file - if visible, folder is expanded
  let nestedFile = page.getByText('nested.md')
  const initiallyVisible = await nestedFile.isVisible()

  // Click to toggle folder state
  await folder.click()

  // Wait for animation
  await page.waitForTimeout(500)

  // Verify state changed
  nestedFile = page.getByText('nested.md')
  const afterFirstClick = await nestedFile.isVisible()
  expect(afterFirstClick).toBe(!initiallyVisible)

  // Click again to toggle back
  await folder.click()
  await page.waitForTimeout(500)

  // Should be back to initial state
  nestedFile = page.getByText('nested.md')
  const afterSecondClick = await nestedFile.isVisible()
  expect(afterSecondClick).toBe(initiallyVisible)

  // Now ensure folder is expanded and click the nested file
  if (!afterSecondClick) {
    await folder.click()
    await page.waitForTimeout(500)
  }

  // Click the nested file
  nestedFile = page.getByText('nested.md')
  await nestedFile.click()

  // Verify content loads
  await page.waitForSelector('h1')
  const heading = page.getByRole('heading', { name: 'Nested Document' })
  await expect(heading).toBeVisible()
})

test('should reload content when file is modified externally', async ({ page }) => {
  await page.goto(SERVER_URL)

  // Wait for file list and click test.md to load initial content
  await page.waitForSelector('.file-list')
  const testFileLink = page.getByText('test.md')
  await testFileLink.waitFor()
  await testFileLink.click()

  // Wait for content to load
  await page.waitForSelector('h1')

  // Verify initial content
  let heading = page.getByRole('heading', { name: 'Test Document' })
  await expect(heading).toBeVisible()

  // Modify the file externally and wait for the reload API call
  const updatePromise = page.waitForResponse(
    (response) => response.url().includes('/api/files/test.md') && response.status() === 200,
    { timeout: 10000 }
  )
  writeFileSync(join(testDir, 'test.md'), '# Updated Document\n\nContent was updated.')
  await updatePromise

  // Verify content was updated
  heading = page.getByRole('heading', { name: 'Updated Document' })
  await expect(heading).toBeVisible()

  const updatedContent = page.getByText('Content was updated.')
  await expect(updatedContent).toBeVisible()
})

test('should detect and list new markdown files', async ({ page }) => {
  await page.goto(SERVER_URL)

  await page.waitForSelector('.file-list')

  // Verify new file doesn't exist yet
  let newFileLink = page.getByText('newfile.md', { exact: true })
  await expect(newFileLink).not.toBeVisible()

  // Create a new file
  writeFileSync(join(testDir, 'newfile.md'), '# New File\n\nNewly created content.')

  // Wait for file watcher to detect the new file
  await page.waitForTimeout(2000)

  // Verify new file appears in sidebar
  newFileLink = page.getByText('newfile.md')
  await expect(newFileLink).toBeVisible()

  // Click and verify content
  await newFileLink.click()
  await page.waitForSelector('h1')

  const heading = page.getByRole('heading', { name: 'New File' })
  await expect(heading).toBeVisible()
})

test('should remove file from sidebar when deleted externally', async ({ page }) => {
  await page.goto(SERVER_URL)

  await page.waitForSelector('.file-list')

  // Create a file to delete
  writeFileSync(join(testDir, 'todelete.md'), '# To Delete\n\nThis will be deleted.')

  // Wait for it to appear
  await page.waitForTimeout(2000)

  let deleteFile = page.getByText('todelete.md')
  await expect(deleteFile).toBeVisible()

  // Delete the file
  unlinkSync(join(testDir, 'todelete.md'))

  // Wait for file watcher to detect removal
  await page.waitForTimeout(2000)

  // Verify file is removed from sidebar
  deleteFile = page.getByText('todelete.md', { exact: true })
  await expect(deleteFile).not.toBeVisible()
})

test('should update sidebar when file is renamed', async ({ page }) => {
  await page.goto(SERVER_URL)

  await page.waitForSelector('.file-list')

  // Create a file to rename
  writeFileSync(join(testDir, 'old-name.md'), '# Old Name\n\nThis will be renamed.')

  // Wait for it to appear
  await page.waitForTimeout(2000)

  let oldFile = page.getByText('old-name.md')
  await expect(oldFile).toBeVisible()

  // Rename the file
  renameSync(join(testDir, 'old-name.md'), join(testDir, 'new-name.md'))

  // Wait for file watcher to detect change
  await page.waitForTimeout(2000)

  // Verify old name is gone
  oldFile = page.getByText('old-name.md', { exact: true })
  await expect(oldFile).not.toBeVisible()

  // Verify new name appears
  const newFile = page.getByText('new-name.md')
  await expect(newFile).toBeVisible()

  // Click and verify content is the same
  await newFile.click()
  await page.waitForSelector('h1')

  const heading = page.getByRole('heading', { name: 'Old Name' })
  await expect(heading).toBeVisible()
})

test('should serve and display static images', async ({ page }) => {
  // Create a markdown file with an image reference
  writeFileSync(
    join(testDir, 'with-image.md'),
    '# Document with Image\n\n![Test Image](test-image.png)'
  )

  await page.goto(SERVER_URL)

  await page.waitForSelector('.file-list')

  // Wait for file to appear and click it
  await page.waitForTimeout(2000)
  const imageDoc = page.getByText('with-image.md')
  await imageDoc.click()

  await page.waitForSelector('h1')

  // Verify the image element exists and loads
  const image = page.getByRole('img', { name: 'Test Image' })
  await expect(image).toBeVisible()

  // Verify image src is correct
  const src = await image.getAttribute('src')
  expect(src).toContain('test-image.png')
})

test('should not have WebSocket connection errors', async ({ page }) => {
  const consoleMessages: string[] = []
  const consoleErrors: string[] = []

  // Capture all console messages
  page.on('console', (msg) => {
    const text = msg.text()
    consoleMessages.push(text)
    if (msg.type() === 'error') {
      consoleErrors.push(text)
    }
  })

  await page.goto(SERVER_URL)

  // Wait for the app to load
  await page.waitForSelector('.file-list', { timeout: 10000 })

  // Navigate between files to exercise WebSocket
  const testFileLink = page.getByText('test.md')
  await testFileLink.waitFor({ timeout: 10000 })
  await testFileLink.click()

  // Wait for content to load
  await page.waitForSelector('h1')

  // Navigate to another file
  const anotherFileLink = page.getByText('another.md')
  await anotherFileLink.waitFor({ timeout: 10000 })
  await anotherFileLink.click()

  // Wait for new content to load
  await page.waitForSelector('h1')

  // Check for WebSocket errors
  const wsErrors = consoleErrors.filter(
    (err) => err.toLowerCase().includes('websocket') || err.toLowerCase().includes('ws://')
  )

  // Also check for specific WebSocket connection errors
  const wsClosedBeforeConnectErrors = consoleMessages.filter((msg) =>
    msg.includes('WebSocket is closed before the connection is established')
  )

  expect(wsErrors).toHaveLength(0)
  expect(wsClosedBeforeConnectErrors).toHaveLength(0)
})

test('should track renamed file after edit and rename', async ({ page }) => {
  // Create a test file
  writeFileSync(join(testDir, 'edit-test.md'), '# Original Content\n\nThis is the original.')

  await page.goto(SERVER_URL)
  await page.waitForSelector('.file-list')

  // Click on the test file
  const fileLink = page.getByText('edit-test.md')
  await fileLink.waitFor({ timeout: 10000 })
  await fileLink.click()

  // Wait for content to load
  await page.waitForSelector('h1')

  // Step 1: Edit the file and wait for the reload to complete
  // Wait 1+ second to ensure file modification timestamp changes
  await new Promise((resolve) => setTimeout(resolve, 1100))

  // Set up a promise to wait for the WebSocket message and reload
  const reloadPromise = page.waitForResponse(
    (response) => response.url().includes('/api/files/edit-test.md'),
    { timeout: 10000 }
  )

  writeFileSync(join(testDir, 'edit-test.md'), '# Edited Content\n\nThis has been edited.')

  // Wait for the API to return the updated content
  await reloadPromise

  // Wait for the DOM to update with the new content
  const heading = page.locator('h1')
  await expect(heading).toHaveText('Edited Content', { timeout: 5000 })

  // Step 2: Rename the file
  const oldPath = join(testDir, 'edit-test.md')
  const newPath = join(testDir, 'renamed-test.md')
  renameSync(oldPath, newPath)

  // Wait for the renamed file to appear in sidebar
  const renamedFileLink = page.getByText('renamed-test.md')
  await renamedFileLink.waitFor({ timeout: 10000 })

  // Verify the old file is gone from sidebar
  await expect(page.getByText('edit-test.md')).not.toBeVisible()

  // Verify we're still viewing the same content (not redirected to root)
  await expect(heading).toHaveText('Edited Content')

  // Verify URL reflects the new filename
  expect(page.url()).toContain('renamed-test.md')
})

test('should handle relative links in nested markdown files', async ({ page }) => {
  // Create a root level file
  writeFileSync(join(testDir, 'root-file.md'), '# Root File\n\nThis is at the root.')

  // Create a subfolder with a file that links to the root file
  mkdirSync(join(testDir, 'subfolder'))
  writeFileSync(
    join(testDir, 'subfolder', 'nested-file.md'),
    '# Nested File\n\n[Link to root file](../root-file.md)'
  )

  await page.goto(SERVER_URL)
  await page.waitForSelector('.file-list')

  // Wait for the subfolder to appear in the file list
  const folder = page.getByText('subfolder', { exact: true })
  await folder.waitFor({ timeout: 10000 })
  await folder.click()

  // Click on the nested file
  const nestedFile = page.getByText('nested-file.md')
  await nestedFile.click()

  await page.waitForSelector('h1')

  // Verify we're viewing the nested file
  let heading = page.getByRole('heading', { name: 'Nested File' })
  await expect(heading).toBeVisible()

  // Click the relative link
  const link = page.getByRole('link', { name: 'Link to root file' })
  await link.click()

  // Wait for navigation
  await page.waitForSelector('h1')

  // Verify we navigated to the root file
  heading = page.getByRole('heading', { name: 'Root File' })
  await expect(heading).toBeVisible()

  const content = page.getByText('This is at the root.')
  await expect(content).toBeVisible()

  // Verify the URL reflects the correct file
  expect(page.url()).toContain('root-file.md')
})

test('should render mermaid diagrams', async ({ page }) => {
  // Create a markdown file with a mermaid diagram
  writeFileSync(
    join(testDir, 'with-mermaid.md'),
    `# Document with Mermaid Diagram

\`\`\`mermaid
graph TD
    A[Start] --> B{Is it working?}
    B -->|Yes| C[Great!]
    B -->|No| D[Debug]
    D --> A
\`\`\`

This is a test mermaid diagram.`
  )

  await page.goto(SERVER_URL)
  await page.waitForSelector('.file-list')

  // Wait for file to appear and click it
  await page.waitForTimeout(2000)
  const mermaidDoc = page.getByText('with-mermaid.md')
  await mermaidDoc.click()

  await page.waitForSelector('h1')

  // Verify the mermaid diagram wrapper exists
  const mermaidWrapper = page.locator('.mermaid-wrapper')
  await expect(mermaidWrapper).toBeVisible()

  // Verify the SVG element is rendered (mermaid renders to SVG)
  const svgElement = page.locator('.mermaid-wrapper svg')
  await expect(svgElement).toBeVisible()

  // Verify some of the text content appears in the diagram
  const diagramContent = page.locator('.mermaid-wrapper')
  await expect(diagramContent).toContainText('Start')
})

test('should change theme when selecting from theme modal', async ({ page }) => {
  await page.goto(SERVER_URL)
  await page.waitForSelector('.file-list')

  // Click the theme toggle button
  const themeToggle = page.locator('.theme-toggle')
  await expect(themeToggle).toBeVisible()
  await themeToggle.click()

  // Wait for theme modal to appear
  const themeModal = page.locator('.theme-modal')
  await expect(themeModal).toBeVisible()

  // Get initial theme from html data attribute
  const initialTheme = await page.locator('html').getAttribute('data-theme')

  // Select a different theme (e.g., if current is 'catppuccin-mocha', select 'light')
  const targetTheme = initialTheme === 'light' ? 'dark' : 'light'
  const themeCard = page.locator(`.theme-card[data-theme="${targetTheme}"]`)
  await themeCard.click()

  // Wait for modal to close
  await expect(themeModal).not.toBeVisible()

  // Verify theme changed in html data attribute (wait for it to apply)
  await page.waitForFunction(
    (expected) => document.documentElement.getAttribute('data-theme') === expected,
    targetTheme,
    { timeout: 5000 }
  )
  const newTheme = await page.locator('html').getAttribute('data-theme')
  expect(newTheme).toBe(targetTheme)

  // Verify theme persists after reload
  await page.reload()
  await page.waitForSelector('.file-list')
  const persistedTheme = await page.locator('html').getAttribute('data-theme')
  expect(persistedTheme).toBe(targetTheme)
})

test('should resize sidebar when dragging resize handle', async ({ page }) => {
  await page.goto(SERVER_URL)
  await page.waitForSelector('.file-list')

  // Get initial sidebar width
  const sidebar = page.locator('.sidebar')
  const initialBox = await sidebar.boundingBox()
  expect(initialBox).not.toBeNull()
  const initialWidth = initialBox!.width

  // Find the resize handle
  const resizeHandle = page.locator('.sidebar-resize-handle')
  await expect(resizeHandle).toBeVisible()

  // Get the handle position
  const handleBox = await resizeHandle.boundingBox()
  expect(handleBox).not.toBeNull()

  // Drag the handle to resize (drag 100px to the right)
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + handleBox!.height / 2)
  await page.mouse.down()
  await page.mouse.move(
    handleBox!.x + handleBox!.width / 2 + 100,
    handleBox!.y + handleBox!.height / 2
  )
  await page.mouse.up()

  // Wait for resize to complete
  await page.waitForTimeout(100)

  // Verify sidebar width increased
  const newBox = await sidebar.boundingBox()
  expect(newBox).not.toBeNull()
  const newWidth = newBox!.width

  // Width should have increased by approximately 100px (allow some tolerance)
  expect(newWidth).toBeGreaterThan(initialWidth + 80)
  expect(newWidth).toBeLessThan(initialWidth + 120)
})

test('should collapse and expand sidebar when clicking toggle button', async ({ page }) => {
  await page.goto(SERVER_URL)
  await page.waitForSelector('.file-list')

  // Get initial sidebar width
  const sidebar = page.locator('.sidebar')
  const initialBox = await sidebar.boundingBox()
  expect(initialBox).not.toBeNull()
  const initialWidth = initialBox!.width

  // Find the sidebar toggle button
  const sidebarToggle = page.locator('.sidebar-toggle')
  await expect(sidebarToggle).toBeVisible()

  // Click to collapse
  await sidebarToggle.click()
  await page.waitForTimeout(300) // Wait for animation

  // Verify sidebar is collapsed (much narrower)
  const collapsedBox = await sidebar.boundingBox()
  expect(collapsedBox).not.toBeNull()
  const collapsedWidth = collapsedBox!.width
  expect(collapsedWidth).toBeLessThan(initialWidth / 2)

  // Verify app has sidebar-collapsed class
  const app = page.locator('.app')
  await expect(app).toHaveClass(/sidebar-collapsed/)

  // Click to expand
  await sidebarToggle.click()
  await page.waitForTimeout(300) // Wait for animation

  // Verify sidebar is expanded again
  const expandedBox = await sidebar.boundingBox()
  expect(expandedBox).not.toBeNull()
  const expandedWidth = expandedBox!.width
  expect(expandedWidth).toBeGreaterThan(collapsedWidth)

  // Verify app no longer has sidebar-collapsed class
  await expect(app).not.toHaveClass(/sidebar-collapsed/)
})
