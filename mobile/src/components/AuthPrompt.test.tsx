import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import AuthPrompt from './AuthPrompt'

describe('AuthPrompt', () => {
  it('renders the sign-in heading', () => {
    render(<AuthPrompt />)
    expect(screen.getByText('Sign in to get started')).toBeInTheDocument()
  })

  it('renders the description text', () => {
    render(<AuthPrompt />)
    expect(
      screen.getByText('Create an account or sign in to record and transcribe your meetings.')
    ).toBeInTheDocument()
  })

  it('renders a Sign In link pointing to /auth/login', () => {
    render(<AuthPrompt />)
    const signInLink = screen.getByText('Sign In')
    expect(signInLink).toBeInTheDocument()
    expect(signInLink.closest('a')).toHaveAttribute('href', '/auth/login')
  })

  it('renders a Create Account link pointing to /auth/register', () => {
    render(<AuthPrompt />)
    const createLink = screen.getByText('Create Account')
    expect(createLink).toBeInTheDocument()
    expect(createLink.closest('a')).toHaveAttribute('href', '/auth/register')
  })

  it('renders user icon SVG', () => {
    render(<AuthPrompt />)
    const svg = document.querySelector('svg')
    expect(svg).toBeInTheDocument()
  })
})
