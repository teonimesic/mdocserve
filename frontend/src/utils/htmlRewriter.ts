/**
 * Extracts the directory part from a file path.
 * "dir/subdir/file.md" -> "dir/subdir"
 * "file.md" -> ""
 */
export function getDirectory(filePath: string): string {
  const lastSlash = filePath.lastIndexOf('/')
  return lastSlash >= 0 ? filePath.substring(0, lastSlash) : ''
}

/**
 * Rewrites relative image src attributes to use /api/static/ with the correct directory context.
 * Images referenced relative to a markdown file need the file's directory prepended.
 *
 * Example: For file "docs/guide/page.md" with image src="diagram.png":
 *   -> src="/api/static/docs/guide/diagram.png"
 */
export function rewriteImagePaths(html: string, filePath: string, cacheBust = false): string {
  const dir = getDirectory(filePath)
  const prefix = dir ? `${dir}/` : ''
  const timestamp = cacheBust ? `?t=${Date.now()}` : ''

  return html.replace(/<img([^>]*)\s+src="([^":/]+[^":]*)"/g, (_match, attrs, src) => {
    // Only rewrite if it's a relative path (doesn't start with http://, https://, or /)
    if (!src.startsWith('http://') && !src.startsWith('https://') && !src.startsWith('/')) {
      return `<img${attrs} src="/api/static/${prefix}${src}${timestamp}"`
    }
    return _match
  })
}

/**
 * Checks if a link href is a relative asset link (not markdown, not external, not anchor).
 * These are files like PDFs, images, etc. that should be served via /api/static/.
 */
export function isRelativeAssetLink(href: string): boolean {
  if (!href) return false

  // Exclude absolute URLs
  if (href.startsWith('http://') || href.startsWith('https://') || href.startsWith('//')) {
    return false
  }

  // Exclude anchors
  if (href.startsWith('#')) {
    return false
  }

  // Exclude markdown links (handled separately for in-app navigation)
  if (href.endsWith('.md') || href.includes('.md#') || href.includes('.md?')) {
    return false
  }

  // Must have a file extension to be considered an asset
  const lastDot = href.lastIndexOf('.')
  const lastSlash = href.lastIndexOf('/')
  return lastDot > lastSlash && lastDot > 0
}
