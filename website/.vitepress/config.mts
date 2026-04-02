import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'workflow',
  description: 'A file-based workflow orchestrator with TUI and CLI',
  base: '/workflow/',

  markdown: {
    config(md) {
      // Wrap inline code containing {{ }} with v-pre to prevent Vue parsing.
      const defaultCodeInline = md.renderer.rules.code_inline!
      md.renderer.rules.code_inline = (tokens, idx, options, env, self) => {
        const token = tokens[idx]
        if (token.content.includes('{{')) {
          return `<code v-pre>${md.utils.escapeHtml(token.content)}</code>`
        }
        return defaultCodeInline(tokens, idx, options, env, self)
      }
    }
  },

  head: [
    ['link', { rel: 'icon', href: '/workflow/favicon.svg' }],
  ],

  themeConfig: {
    logo: '/favicon.svg',
    siteTitle: 'workflow',

    nav: [
      { text: 'Guide', link: '/guide/introduction' },
      { text: 'Features', link: '/features/notifications' },
      { text: 'Reference', link: '/reference/cli' },
      {
        text: 'v0.4.2',
        items: [
          { text: 'Changelog', link: 'https://github.com/vchaindz/workflow/blob/main/CHANGELOG.md' },
          { text: 'Releases', link: 'https://github.com/vchaindz/workflow/releases' },
        ]
      }
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Introduction', link: '/guide/introduction' },
            { text: 'Installation', link: '/guide/installation' },
            { text: 'Quick Start', link: '/guide/quick-start' },
          ]
        },
        {
          text: 'Core Concepts',
          items: [
            { text: 'Workflows & YAML', link: '/guide/workflows' },
            { text: 'TUI Interface', link: '/guide/tui' },
            { text: 'AI Integration', link: '/guide/ai-integration' },
            { text: 'Configuration', link: '/guide/configuration' },
          ]
        }
      ],
      '/features/': [
        {
          text: 'Features',
          items: [
            { text: 'Notifications', link: '/features/notifications' },
            { text: 'Expressions & Loops', link: '/features/expressions' },
            { text: 'Encrypted Secrets', link: '/features/secrets' },
            { text: 'MCP Integration', link: '/features/mcp' },
            { text: 'Memory & Anomalies', link: '/features/memory' },
            { text: 'Sync & Snapshots', link: '/features/sync-and-snapshots' },
            { text: 'Webhook Server', link: '/features/webhooks' },
          ]
        }
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'CLI Commands', link: '/reference/cli' },
            { text: 'YAML Schema', link: '/reference/yaml-schema' },
            { text: 'Template Catalog', link: '/reference/templates' },
          ]
        }
      ],
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/vchaindz/workflow' }
    ],

    search: {
      provider: 'local',
      options: {
        translations: {
          button: { buttonText: 'Search', buttonAriaLabel: 'Search' },
          modal: {
            noResultsText: 'No results for',
            resetButtonTitle: 'Reset search',
            footer: { selectText: 'to select', navigateText: 'to navigate', closeText: 'to close' }
          }
        }
      }
    },

    editLink: {
      pattern: 'https://github.com/vchaindz/workflow/edit/main/website/:path',
      text: 'Edit this page on GitHub'
    },

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright 2026 Dennis Zimmer'
    }
  }
})
