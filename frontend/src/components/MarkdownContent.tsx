import { ReactNode, createElement, memo } from 'react'
import { CodeBlock } from './CodeBlock'
import { MermaidDiagram } from './MermaidDiagram'
import { TodoCheckbox } from './TodoCheckbox'
import { resolveRelativePath, isRelativeMarkdownLink } from '../utils/pathResolver'
import { isRelativeAssetLink, getDirectory } from '../utils/htmlRewriter'

interface MarkdownContentProps {
  html: string
  filePath: string
  theme?: string
  onLinkClick?: (path: string) => void
}

// Convert CSS string to React style object
function parseStyleString(styleStr: string): Record<string, string> {
  const styleObj: Record<string, string> = {}

  styleStr.split(';').forEach((rule) => {
    const [property, value] = rule.split(':').map((s) => s.trim())
    if (property && value) {
      // Convert kebab-case to camelCase (e.g., background-color -> backgroundColor)
      const camelProperty = property.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())
      styleObj[camelProperty] = value
    }
  })

  return styleObj
}

// Memoized MarkdownContent - only re-renders if html, filePath, or theme change
// This prevents unnecessary re-renders when sidebar state changes
export const MarkdownContent = memo(function MarkdownContent({
  html,
  filePath,
  theme,
  onLinkClick,
}: MarkdownContentProps) {
  const parser = new DOMParser()
  const doc = parser.parseFromString(html, 'text/html')

  let todoIndex = 0

  const convertNodeToReact = (node: Node, key: number): ReactNode => {
    // Text nodes
    if (node.nodeType === Node.TEXT_NODE) {
      return node.textContent
    }

    // Element nodes
    if (node.nodeType === Node.ELEMENT_NODE) {
      const element = node as Element
      const tagName = element.tagName.toLowerCase()

      // Handle code blocks
      if (tagName === 'pre') {
        const codeElement = element.querySelector('code')
        if (codeElement) {
          const className = codeElement.className
          const languageMatch = className.match(/language-(\w+)/)

          if (languageMatch) {
            const language = languageMatch[1] || 'text'
            const code = codeElement.textContent || ''

            // Mermaid diagrams
            if (language === 'mermaid') {
              return <MermaidDiagram key={key} chart={code} theme={theme} />
            }

            // Regular code blocks
            return <CodeBlock key={key} code={code} language={language} />
          }
        }
      }

      // Handle task list items with checkboxes (marked already converts them to input elements)
      if (tagName === 'li' && element.querySelector('input[type="checkbox"]')) {
        const checkbox = element.querySelector('input[type="checkbox"]')
        if (checkbox) {
          const isChecked = checkbox.hasAttribute('checked')
          const currentIndex = todoIndex++

          // Get all children except the checkbox (marked puts it as first child)
          const children = Array.from(element.childNodes)
            .filter((child) => child !== checkbox)
            .map((child, i) => convertNodeToReact(child, i))
            .filter((child) => child !== '' && child !== null)

          return (
            <li key={key}>
              <TodoCheckbox checked={isChecked} index={currentIndex} filePath={filePath} />{' '}
              {children}
            </li>
          )
        }
      }

      // Handle anchor tags
      if (tagName === 'a') {
        const href = element.getAttribute('href')

        // Handle relative markdown links (in-app navigation)
        if (href && onLinkClick && isRelativeMarkdownLink(href)) {
          const resolvedPath = resolveRelativePath(filePath, href)

          // Get all children
          const children = Array.from(element.childNodes).map((child, i) =>
            convertNodeToReact(child, i)
          )

          // Get other attributes
          const props: Record<string, unknown> = { key }
          for (let i = 0; i < element.attributes.length; i++) {
            const attr = element.attributes[i]
            if (attr && attr.name !== 'href') {
              if (attr.name === 'class') {
                props.className = attr.value
              } else if (attr.name === 'style') {
                props.style = parseStyleString(attr.value)
              } else {
                props[attr.name] = attr.value
              }
            }
          }

          // Add click handler to intercept navigation
          props.href = href
          props.onClick = (e: React.MouseEvent) => {
            e.preventDefault()
            onLinkClick(resolvedPath)
          }

          return createElement('a', props, ...children)
        }

        // Handle relative asset links (PDFs, images, etc.) - rewrite to /api/static/
        if (href && isRelativeAssetLink(href)) {
          const dir = getDirectory(filePath)
          const prefix = dir ? `${dir}/` : ''
          const staticHref = `/api/static/${prefix}${href}`

          const children = Array.from(element.childNodes).map((child, i) =>
            convertNodeToReact(child, i)
          )

          const props: Record<string, unknown> = { key }
          for (let i = 0; i < element.attributes.length; i++) {
            const attr = element.attributes[i]
            if (attr && attr.name !== 'href') {
              if (attr.name === 'class') {
                props.className = attr.value
              } else if (attr.name === 'style') {
                props.style = parseStyleString(attr.value)
              } else {
                props[attr.name] = attr.value
              }
            }
          }

          props.href = staticHref
          props.target = '_blank'
          props.rel = 'noopener noreferrer'

          return createElement('a', props, ...children)
        }
      }

      // Regular elements - convert children recursively
      const children = Array.from(element.childNodes).map((child, i) =>
        convertNodeToReact(child, i)
      )

      // Get attributes
      const props: Record<string, unknown> = { key }
      for (let i = 0; i < element.attributes.length; i++) {
        const attr = element.attributes[i]
        if (attr) {
          // Convert class to className for React
          if (attr.name === 'class') {
            props.className = attr.value
          }
          // Convert style string to object for React
          else if (attr.name === 'style') {
            props.style = parseStyleString(attr.value)
          }
          // All other attributes
          else {
            props[attr.name] = attr.value
          }
        }
      }

      // Create React element using createElement
      return createElement(tagName, props, ...children)
    }

    return null
  }

  // Convert body children to React elements
  const reactElements = Array.from(doc.body.childNodes).map((node, i) =>
    convertNodeToReact(node, i)
  )

  return <div className="markdown-content">{reactElements}</div>
})
