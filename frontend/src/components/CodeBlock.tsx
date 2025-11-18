import { useEffect, useRef, useState } from 'react'
import Prism from 'prismjs'
import 'prismjs/themes/prism-tomorrow.css'
import 'prismjs/components/prism-javascript'
import 'prismjs/components/prism-typescript'
import 'prismjs/components/prism-python'
import 'prismjs/components/prism-rust'
import 'prismjs/components/prism-bash'
import 'prismjs/components/prism-go'
import 'prismjs/components/prism-json'
import 'prismjs/components/prism-sql'
import 'prismjs/components/prism-css'
import 'prismjs/components/prism-markup'
import 'prismjs/plugins/line-numbers/prism-line-numbers.css'
import 'prismjs/plugins/line-numbers/prism-line-numbers'
import './CodeBlock.css'

interface CodeBlockProps {
  code: string
  language: string
}

export function CodeBlock({ code, language }: CodeBlockProps) {
  const codeRef = useRef<HTMLElement>(null)
  const [showLineNumbers, setShowLineNumbers] = useState(() => {
    // Check sessionStorage, default to false (off)
    return sessionStorage.getItem('code-line-numbers') === 'true'
  })
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (codeRef.current) {
      // Remove any existing line-numbers-rows element before re-highlighting
      const pre = codeRef.current.parentElement
      if (pre) {
        const existingLineNumbers = pre.querySelector('.line-numbers-rows')
        if (existingLineNumbers) {
          existingLineNumbers.remove()
        }
      }

      Prism.highlightElement(codeRef.current)

      // Manually add line number text content (Prism creates empty spans)
      if (showLineNumbers && pre) {
        const lineNumbersRows = pre.querySelector('.line-numbers-rows')
        if (lineNumbersRows) {
          const spans = lineNumbersRows.querySelectorAll('span')
          spans.forEach((span, index) => {
            span.textContent = String(index + 1)
          })
        }
      }
    }
  }, [code, language, showLineNumbers])

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }

  const toggleLineNumbers = () => {
    const newValue = !showLineNumbers
    setShowLineNumbers(newValue)
    sessionStorage.setItem('code-line-numbers', newValue.toString())
  }

  // Map common language aliases to Prism language names
  const getPrismLanguage = (lang: string): string => {
    const langMap: Record<string, string> = {
      'js': 'javascript',
      'ts': 'typescript',
      'py': 'python',
      'rs': 'rust',
      'sh': 'bash',
      'html': 'markup',
      'xml': 'markup',
    }
    return langMap[lang] || lang
  }

  const prismLanguage = getPrismLanguage(language)
  const lineNumberClass = showLineNumbers ? 'line-numbers' : ''

  return (
    <div className="code-block-wrapper">
      <div className="code-block-header">
        <span className="code-block-language">{language}</span>
        <div className="code-block-actions">
          <button
            className={`code-block-btn ${showLineNumbers ? 'active' : ''}`}
            onClick={toggleLineNumbers}
            aria-label="Toggle line numbers"
            title="Toggle line numbers"
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="4" y1="3" x2="12" y2="3"/>
              <line x1="4" y1="8" x2="12" y2="8"/>
              <line x1="4" y1="13" x2="12" y2="13"/>
              <circle cx="1.5" cy="3" r="0.5" fill="currentColor"/>
              <circle cx="1.5" cy="8" r="0.5" fill="currentColor"/>
              <circle cx="1.5" cy="13" r="0.5" fill="currentColor"/>
            </svg>
          </button>
          <button
            className={`code-block-btn ${copied ? 'copied' : ''}`}
            onClick={handleCopy}
            aria-label="Copy code"
            title={copied ? 'Copied!' : 'Copy code'}
          >
            {copied ? (
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2">
                <polyline points="3,8 6,11 13,4"/>
              </svg>
            ) : (
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="4" y="4" width="8" height="10" rx="1"/>
                <path d="M8 4V2a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1h-2"/>
              </svg>
            )}
          </button>
        </div>
      </div>
      <pre className={lineNumberClass}>
        <code ref={codeRef} className={`language-${prismLanguage}`}>
          {code}
        </code>
      </pre>
    </div>
  )
}
