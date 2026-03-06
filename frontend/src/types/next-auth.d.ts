
import "next-auth";

declare module "next-auth" {
  interface Session {
    accessToken?: string;
    idToken?: string;
    error?: string;
    calendarScopes?: boolean;
    refreshToken?: string;
    rawScopes?: string;
    user: {
      email?: string;
      name?: string;
      image?: string;
    };
  }
}

declare module "next-auth/jwt" {
  interface JWT {
    accessToken?: string;
    idToken?: string;
    error?: string;
    calendarScopes?: boolean;
    refreshToken?: string;
    rawScopes?: string;
  }
}
