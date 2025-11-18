import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { CodeBlock } from '../CodeBlock'

describe('CodeBlock', () => {
  const sampleCode = 'const foo = "bar";'

  it('renders code with correct language class', () => {
    const { container } = render(<CodeBlock code={sampleCode} language="javascript" />)
    
    const codeElement = container.querySelector('code')
    expect(codeElement).toHaveClass('language-javascript')
    expect(codeElement?.textContent).toBe(sampleCode)
  })

  it('displays language label', () => {
    render(<CodeBlock code={sampleCode} language="typescript" />)
    
    expect(screen.getByText('typescript')).toBeInTheDocument()
  })

  it('has toggle line numbers button', () => {
    render(<CodeBlock code={sampleCode} language="javascript" />)
    
    const toggleButton = screen.getByRole('button', { name: /toggle line numbers/i })
    expect(toggleButton).toBeInTheDocument()
  })

  it('has copy button', () => {
    render(<CodeBlock code={sampleCode} language="javascript" />)
    
    const copyButton = screen.getByRole('button', { name: /copy code/i })
    expect(copyButton).toBeInTheDocument()
  })

  it('toggles line numbers when clicked', async () => {
    const user = userEvent.setup()
    const { container } = render(<CodeBlock code={sampleCode} language="javascript" />)
    
    const toggleButton = screen.getByRole('button', { name: /toggle line numbers/i })
    const preElement = container.querySelector('pre')
    
    // Initially no line numbers (based on default sessionStorage)
    expect(preElement).not.toHaveClass('line-numbers')
    
    // Click to enable
    await user.click(toggleButton)
    await waitFor(() => {
      expect(preElement).toHaveClass('line-numbers')
    })
    
    // Click to disable
    await user.click(toggleButton)
    await waitFor(() => {
      expect(preElement).not.toHaveClass('line-numbers')
    })
  })

  it('copies code to clipboard when copy button is clicked', async () => {
    const user = userEvent.setup()
    const writeText = vi.fn().mockResolvedValue(undefined)

    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      writable: true,
      configurable: true,
    })

    render(<CodeBlock code={sampleCode} language="javascript" />)

    const copyButton = screen.getByRole('button', { name: /copy code/i })
    await user.click(copyButton)

    expect(writeText).toHaveBeenCalledWith(sampleCode)
  })

  it('maps language aliases correctly', () => {
    const { container } = render(<CodeBlock code={sampleCode} language="js" />)
    
    const codeElement = container.querySelector('code')
    // Should map 'js' to 'javascript'
    expect(codeElement).toHaveClass('language-javascript')
  })

  it('persists line numbers state to sessionStorage', async () => {
    const user = userEvent.setup()
    sessionStorage.clear()

    render(<CodeBlock code={sampleCode} language="javascript" />)

    const toggleButton = screen.getByRole('button', { name: /toggle line numbers/i })
    await user.click(toggleButton)

    expect(sessionStorage.getItem('code-line-numbers')).toBe('true')
  })

  it('applies line-numbers class correctly for multi-line code', async () => {
    const user = userEvent.setup()
    const multiLineCode = 'line 1\nline 2\nline 3\nline 4\nline 5'
    const { container } = render(<CodeBlock code={multiLineCode} language="javascript" />)

    const toggleButton = screen.getByRole('button', { name: /toggle line numbers/i })
    const preElement = container.querySelector('pre')

    // Enable line numbers
    await user.click(toggleButton)

    await waitFor(() => {
      expect(preElement).toHaveClass('line-numbers')
    })

    // Prism should have applied line numbers
    // The line-numbers plugin adds data-line attributes or line number spans
    // We verify the class is present which enables the CSS styling
    expect(preElement?.classList.contains('line-numbers')).toBe(true)
  })

  it('handles clipboard copy error gracefully', async () => {
    const user = userEvent.setup()
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    Object.defineProperty(navigator, 'clipboard', {
      value: {
        writeText: vi.fn().mockRejectedValue(new Error('Clipboard error'))
      },
      writable: true,
      configurable: true,
    })

    render(<CodeBlock code={sampleCode} language="javascript" />)

    const copyButton = screen.getByRole('button', { name: /copy code/i })
    await user.click(copyButton)

    await waitFor(() => {
      expect(consoleErrorSpy).toHaveBeenCalledWith('Failed to copy:', expect.any(Error))
    })

    consoleErrorSpy.mockRestore()
  })
})
