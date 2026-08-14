import { defineConfig } from 'vitepress';

export default defineConfig({
  title: 'ReBackUp',
  description: 'Selectively back up critical data before a Windows clean install.',
  lastUpdated: true,
  cleanUrls: true,
  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'Security', link: '/guide/security' },
      { text: 'GitHub', link: 'https://github.com/Aryansingh0783/rebackup' },
    ],
    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'Getting started', link: '/guide/getting-started' },
          { text: 'Scanning drives', link: '/guide/scanning' },
          { text: 'Profiles', link: '/guide/profiles' },
          { text: 'Running a backup', link: '/guide/backup' },
          { text: 'Restoring', link: '/guide/restore' },
        ],
      },
      {
        text: 'Reference',
        items: [
          { text: 'Security model', link: '/guide/security' },
          { text: 'Manifest format', link: '/guide/manifest' },
          { text: 'CLI', link: '/guide/cli' },
          { text: 'Troubleshooting', link: '/guide/troubleshooting' },
        ],
      },
    ],
    search: { provider: 'local' },
    footer: { message: 'MIT licensed', copyright: 'Local-only. Nothing leaves your machine.' },
  },
});
