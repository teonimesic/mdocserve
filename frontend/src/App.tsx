import { useState, useEffect, useCallback, useMemo, useRef } from 'react'
import { marked } from './config/marked'
import { useTheme } from './hooks/useTheme'
import { useFolderState } from './hooks/useFolderState'
import { useSidebarResize } from './hooks/useSidebarResize'
import { useSidebarCollapse } from './hooks/useSidebarCollapse'
import { ThemeModal } from './components/ThemeModal'
import { ThemeToggle } from './components/ThemeToggle'
import { SidebarToggle } from './components/SidebarToggle'
import { SidebarResizeHandle } from './components/SidebarResizeHandle'
import { FileTree } from './components/FileTree'
import { MarkdownContent } from './components/MarkdownContent'
import { buildFileTree, type ApiFile } from './utils/fileTree'
import './App.css'
import './CodeBlockEnhancements.css'

// Helper function to wrap tables in div for horizontal scrolling
function wrapTablesForScroll(html: string): string {
  return html
    .replace(/<table>/g, '<div class="table-wrapper"><table>')
    .replace(/<\/table>/g, '</table></div>')
}

// Helper function to rewrite relative image paths to use /api/static/
function rewriteImagePaths(html: string, cacheBust = false): string {
  // Rewrite relative image paths to use /api/static/
  // This handles src="image.png" -> src="/api/static/image.png"
  // And src="path/to/image.png" -> src="/api/static/path/to/image.png"
  // Add cache-busting parameter if needed (for image reloads)
  const timestamp = cacheBust ? `?t=${Date.now()}` : ''
  return html.replace(/<img([^>]*)\s+src="([^":/]+[^":]*)"/g, (match, attrs, src) => {
    // Only rewrite if it's a relative path (doesn't start with http://, https://, or /)
    if (!src.startsWith('http://') && !src.startsWith('https://') && !src.startsWith('/')) {
      return `<img${attrs} src="/api/static/${src}${timestamp}"`
    }
    return match
  })
}

function App() {
  const { theme, setTheme, isThemeModalOpen, openThemeModal, closeThemeModal } = useTheme()
  const { isExpanded, toggleFolder, expandFolder } = useFolderState()
  const { width, setWidth, isResizing, startResizing, stopResizing } = useSidebarResize()
  const { isCollapsed, toggle: toggleSidebar, collapsedWidth } = useSidebarCollapse()
  const [files, setFiles] = useState<ApiFile[]>([])
  const [content, setContent] = useState('')
  const [currentPath, setCurrentPath] = useState('')

  const fileTree = useMemo(() => buildFileTree(files), [files])
  const sidebarRef = useRef<HTMLDivElement>(null)
  const contentRef = useRef<HTMLDivElement>(null)
  const startWidthRef = useRef(width)
  const currentPathRef = useRef(currentPath)
  const filesRef = useRef<ApiFile[]>(files)

  // Update filesRef when files state changes
  useEffect(() => {
    filesRef.current = files
  }, [files])

  const loadFile = useCallback(
    (path: string, cacheBustImages = false) => {
      fetch(`/api/files/${path}`)
        .then((res) => res.json())
        .then((data) => {
          // Parse markdown to HTML client-side
          let html = marked.parse(data.markdown) as string
          // Wrap tables for horizontal scrolling
          html = wrapTablesForScroll(html)
          // Rewrite relative image paths to use /api/static/
          html = rewriteImagePaths(html, cacheBustImages)
          setContent(html)
          setCurrentPath(path)
          currentPathRef.current = path
          // Update URL hash to reflect current file
          window.location.hash = path
          // Auto-expand folders containing the active file
          const parts = path.split('/')
          for (let i = 1; i < parts.length; i++) {
            const folderPath = parts.slice(0, i).join('/')
            expandFolder(folderPath)
          }
        })
        .catch((err) => {
          console.error('Error loading file:', err)
        })
    },
    [expandFolder]
  )

  useEffect(() => {
    // Fetch file list
    fetch('/api/files')
      .then((res) => res.json())
      .then((data) => {
        setFiles(data.files)
        // Check if there's a file path in the URL hash
        const hash = window.location.hash.slice(1) // Remove the '#' prefix
        if (hash) {
          // Try to load the file from the URL
          const fileExists = data.files.some((f: ApiFile) => f.path === hash)
          if (fileExists) {
            loadFile(hash)
          } else if (data.files.length > 0) {
            // Fall back to first file if the URL file doesn't exist
            loadFile(data.files[0].path)
          }
        } else if (data.files.length > 0) {
          // No URL hash, load first file
          loadFile(data.files[0].path)
        }
      })
      .catch((err) => {
        console.error('Error fetching files:', err)
      })
  }, [loadFile])

  useEffect(() => {
    let ws: WebSocket | null = null
    let reconnectTimeout: ReturnType<typeof setTimeout> | null = null

    const connect = () => {
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
      const wsUrl = `${protocol}//${window.location.host}/ws`
      ws = new WebSocket(wsUrl)

      ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data)
          if (message.type === 'Reload') {
            // Reload current file with cache-busting for images
            if (currentPathRef.current) {
              loadFile(currentPathRef.current, true)
            }
          } else if (message.type === 'FileModified') {
            // Handle file modification - reload the specific file
            if (currentPathRef.current === message.name) {
              loadFile(message.name, true)
            }
          } else if (message.type === 'FileAdded') {
            // Handle new file added - refresh file list to show new file
            fetch('/api/files')
              .then((res) => res.json())
              .then((data) => {
                setFiles(data.files)
              })
          } else if (message.type === 'FileRenamed') {
            // Handle file rename
            if (currentPathRef.current === message.old_name) {
              loadFile(message.new_name)
            }
            // Refresh file list
            fetch('/api/files')
              .then((res) => res.json())
              .then((data) => {
                setFiles(data.files)
              })
          } else if (message.type === 'FileRemoved') {
            // Handle file removal
            if (currentPathRef.current === message.name) {
              // Current file was removed - use filesRef to get the old file list
              const oldFiles = new Set(filesRef.current.map((f: ApiFile) => f.path))

              // Fetch updated file list
              fetch('/api/files')
                .then((res) => res.json())
                .then((data) => {
                  const newFiles = data.files.map((f: ApiFile) => f.path)
                  const addedFiles = newFiles.filter((path: string) => !oldFiles.has(path))

                  setFiles(data.files)

                  // If exactly one file was added, it's likely a rename - load it
                  if (addedFiles.length === 1) {
                    loadFile(addedFiles[0])
                  } else if (data.files.length > 0) {
                    // Otherwise load first file
                    loadFile(data.files[0].path)
                  }
                })
            } else {
              // Just refresh the file list
              fetch('/api/files')
                .then((res) => res.json())
                .then((data) => {
                  setFiles(data.files)
                })
            }
          }
        } catch (error) {
          console.error('WebSocket message handler error:', error)
        }
      }

      ws.onerror = (error) => {
        console.error('WebSocket error:', error)
      }

      ws.onclose = () => {
        // Reconnect gracefully without full page reload
        reconnectTimeout = setTimeout(() => {
          connect()
        }, 2000)
      }
    }

    connect()

    return () => {
      if (reconnectTimeout) {
        clearTimeout(reconnectTimeout)
      }
      if (ws) {
        ws.close()
      }
    }
  }, [loadFile])

  const handleResizeStart = () => {
    startResizing()
    startWidthRef.current = width
    if (sidebarRef.current) sidebarRef.current.classList.add('resizing')
    if (contentRef.current) contentRef.current.classList.add('resizing')
  }

  const handleResize = (deltaX: number) => {
    if (isResizing) {
      const newWidth = startWidthRef.current + deltaX
      setWidth(newWidth, false)
    }
  }

  const handleResizeEnd = () => {
    stopResizing()
    if (sidebarRef.current) sidebarRef.current.classList.remove('resizing')
    if (contentRef.current) contentRef.current.classList.remove('resizing')
  }

  // Update CSS custom properties based on sidebar state
  useEffect(() => {
    const currentWidth = isCollapsed ? collapsedWidth : width
    document.documentElement.style.setProperty('--sidebar-width', `${currentWidth}px`)
  }, [width, isCollapsed, collapsedWidth])

  return (
    <div className={`app ${isCollapsed ? 'sidebar-collapsed' : ''}`}>
      <SidebarToggle isCollapsed={isCollapsed} onClick={toggleSidebar} />
      <ThemeToggle onClick={openThemeModal} />
      <ThemeModal
        isOpen={isThemeModalOpen}
        currentTheme={theme}
        onClose={closeThemeModal}
        onSelectTheme={setTheme}
      />
      <div ref={sidebarRef} className="sidebar">
        <div className="sidebar-header"></div>
        <div className="sidebar-content">
          <FileTree
            nodes={fileTree}
            currentPath={currentPath}
            onFileSelect={loadFile}
            isExpanded={isExpanded}
            onToggleFolder={toggleFolder}
          />
        </div>
        {!isCollapsed && (
          <SidebarResizeHandle
            onResize={handleResize}
            onResizeStart={handleResizeStart}
            onResizeEnd={handleResizeEnd}
          />
        )}
      </div>
      <div ref={contentRef} className="content">
        {content && (
          <MarkdownContent
            html={content}
            filePath={currentPath}
            theme={theme}
            onLinkClick={loadFile}
          />
        )}
      </div>
    </div>
  )
}

export default App
