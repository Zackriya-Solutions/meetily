import { NextAuthOptions } from "next-auth";
import GoogleProvider from "next-auth/providers/google";
import { JWT } from "next-auth/jwt";

/**
 * Helper to refresh Google access token
 */
async function refreshAccessToken(token: JWT) {
  try {
    const url = "https://oauth2.googleapis.com/token";
    
    const response = await fetch(url, {
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        client_id: process.env.GOOGLE_CLIENT_ID!,
        client_secret: process.env.GOOGLE_CLIENT_SECRET!,
        grant_type: "refresh_token",
        refresh_token: token.refreshToken as string,
      }),
      method: "POST",
    });

    const refreshedTokens = await response.json();

    if (!response.ok) {
      throw refreshedTokens;
    }

    return {
      ...token,
      accessToken: refreshedTokens.access_token,
      accessTokenExpires: Date.now() + refreshedTokens.expires_in * 1000,
      refreshToken: refreshedTokens.refresh_token ?? token.refreshToken, // Fall back to old refresh token
      idToken: refreshedTokens.id_token ?? token.idToken,
    };
  } catch (error) {
    console.error("[Auth] Error refreshing access token", error);

    return {
      ...token,
      error: "RefreshAccessTokenError",
    };
  }
}

/**
 * NextAuth.js Configuration
 * - Google OAuth with @appointy.com domain restriction
 * - JWT session strategy for backend API calls
 * - Automatic token refresh rotation
 */
export const authOptions: NextAuthOptions = {
  providers: [
    GoogleProvider({
      clientId: process.env.GOOGLE_CLIENT_ID!,
      clientSecret: process.env.GOOGLE_CLIENT_SECRET!,
      authorization: {
        params: {
          prompt: "consent",
          access_type: "offline",
          response_type: "code",
          scope: "openid email profile https://www.googleapis.com/auth/calendar.readonly https://www.googleapis.com/auth/calendar.events",
        },
      },
    }),
  ],
  
  callbacks: {
    // Domain restriction - only allow @appointy.com
    async signIn({ user }) {
      const allowedDomains = ['appointy.com'];
      const email = user.email || '';
      const domain = email.split('@')[1];
      
      if (!allowedDomains.includes(domain)) {
        console.log(`[Auth] Rejected login from: ${email}`);
        return false;
      }
      
      console.log(`[Auth] Successful login: ${email}`);
      return true;
    },
    
    // Include user info in JWT token and handle refresh
    async jwt({ token, account, user }) {
      // Initial sign in
      if (account && user) {
          // Calculate expiry time (handle both expires_at and expires_in)
          const accessTokenExpires = account.expires_at 
            ? account.expires_at * 1000 
            : Date.now() + (Number(account?.expires_in) || 3600) * 1000;
            
          console.log('[Auth] Initial sign in - Token expires at:', new Date(accessTokenExpires).toISOString(), 'expires_in:', account.expires_in);
          
          
                    
          return {
            accessToken: account.access_token,
            accessTokenExpires,
            refreshToken: account.refresh_token,
            idToken: account.id_token,
            email: user.email,
            name: user.name,
            picture: user.image,
            calendarScopes: account.scope?.includes('calendar'),
            rawScopes: account.scope,
          };
      }

      // Return previous token if the access token has not expired yet (with 5-minute buffer)
      // Refresh 5 minutes before actual expiry to prevent race conditions during API calls
      if (Date.now() < (token.accessTokenExpires as number) - 5 * 60 * 1000) {
        return token;
      }

      // Access token has expired, try to update it
      console.log("[Auth] Access token expired, refreshing...");
      return refreshAccessToken(token);
    },
    
    // Make token info available in session
    async session({ session, token }) {
      session.accessToken = token.accessToken as string;
      session.idToken = token.idToken as string;
      session.error = token.error as string;
      session.calendarScopes = token.calendarScopes as boolean;
      session.refreshToken = token.refreshToken as string;
      session.rawScopes = token.rawScopes as string;
      session.user = {
        ...session.user,
        email: token.email as string,
        name: token.name as string,
        image: token.picture as string,
      };
      return session;
    },
  },
  
  pages: {
    signIn: '/login',
    error: '/login',
  },
  
  session: {
    strategy: 'jwt',
    maxAge: 3 * 24 * 60 * 60, // 3 days
  },
  
  debug: process.env.NODE_ENV === 'development',
};
