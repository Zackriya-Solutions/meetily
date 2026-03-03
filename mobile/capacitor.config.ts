import type { CapacitorConfig } from '@capacitor/cli'

const config: CapacitorConfig = {
  appId: 'com.iqcapture.mobile',
  appName: 'IQ:capture',
  webDir: 'out',
  server: {
    // During development, point to Next.js dev server
    // url: 'http://localhost:3119',
    // cleartext: true,
  },
  plugins: {
    CapacitorSQLite: {
      iosDatabaseLocation: 'Library/CapacitorDatabase',
      iosIsEncryption: false,
      androidIsEncryption: false,
    },
  },
}

export default config
