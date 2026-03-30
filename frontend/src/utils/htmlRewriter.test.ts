import { describe, it, expect } from 'vitest'
import { getDirectory, rewriteImagePaths, isRelativeAssetLink } from './htmlRewriter'

describe('getDirectory', () => {
  it('should extract directory from nested path', () => {
    expect(getDirectory('dir/subdir/file.md')).toBe('dir/subdir')
  })

  it('should extract directory from single level path', () => {
    expect(getDirectory('dir/file.md')).toBe('dir')
  })

  it('should return empty string for root-level file', () => {
    expect(getDirectory('file.md')).toBe('')
  })

  it('should handle deeply nested paths', () => {
    expect(getDirectory('a/b/c/d/file.md')).toBe('a/b/c/d')
  })
})

describe('rewriteImagePaths', () => {
  it('should prepend directory from file path to relative image src', () => {
    const html = '<img alt="test" src="image.png">'
    const result = rewriteImagePaths(html, 'dir/subdir/page.md')
    expect(result).toBe('<img alt="test" src="/api/static/dir/subdir/image.png">')
  })

  it('should handle file at root level (no directory prefix)', () => {
    const html = '<img alt="test" src="image.png">'
    const result = rewriteImagePaths(html, 'page.md')
    expect(result).toBe('<img alt="test" src="/api/static/image.png">')
  })

  it('should handle image with nested relative path', () => {
    const html = '<img alt="test" src="assets/image.png">'
    const result = rewriteImagePaths(html, 'docs/guide/page.md')
    expect(result).toBe('<img alt="test" src="/api/static/docs/guide/assets/image.png">')
  })

  it('should not rewrite absolute URLs', () => {
    const html = '<img alt="test" src="https://example.com/image.png">'
    const result = rewriteImagePaths(html, 'dir/page.md')
    expect(result).toBe('<img alt="test" src="https://example.com/image.png">')
  })

  it('should not rewrite paths starting with /', () => {
    const html = '<img alt="test" src="/api/static/already/correct.png">'
    const result = rewriteImagePaths(html, 'dir/page.md')
    expect(result).toBe('<img alt="test" src="/api/static/already/correct.png">')
  })

  it('should handle multiple images', () => {
    const html = '<img src="a.png"><img src="b.jpg">'
    const result = rewriteImagePaths(html, 'docs/page.md')
    expect(result).toBe('<img src="/api/static/docs/a.png"><img src="/api/static/docs/b.jpg">')
  })

  it('should add cache-busting parameter when requested', () => {
    const html = '<img alt="test" src="image.png">'
    const result = rewriteImagePaths(html, 'dir/page.md', true)
    expect(result).toMatch(/src="\/api\/static\/dir\/image\.png\?t=\d+"/)
  })
})

describe('isRelativeAssetLink', () => {
  it('should return true for PDF links', () => {
    expect(isRelativeAssetLink('document.pdf')).toBe(true)
  })

  it('should return true for image links', () => {
    expect(isRelativeAssetLink('photo.jpg')).toBe(true)
  })

  it('should return true for nested asset paths', () => {
    expect(isRelativeAssetLink('assets/doc.pdf')).toBe(true)
  })

  it('should return false for markdown links', () => {
    expect(isRelativeAssetLink('page.md')).toBe(false)
  })

  it('should return false for markdown links with anchors', () => {
    expect(isRelativeAssetLink('page.md#section')).toBe(false)
  })

  it('should return false for external URLs', () => {
    expect(isRelativeAssetLink('https://example.com/doc.pdf')).toBe(false)
    expect(isRelativeAssetLink('http://example.com/doc.pdf')).toBe(false)
  })

  it('should return false for anchor links', () => {
    expect(isRelativeAssetLink('#section')).toBe(false)
  })

  it('should return false for empty string', () => {
    expect(isRelativeAssetLink('')).toBe(false)
  })

  it('should return false for paths without extension', () => {
    expect(isRelativeAssetLink('somefile')).toBe(false)
  })
})
