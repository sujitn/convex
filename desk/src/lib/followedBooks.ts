import { useQuery } from '@tanstack/react-query'
import { useSession } from '@convex/api-client'
import { useCallback, useMemo } from 'react'
import { useSearchParams } from 'react-router-dom'
import { readLiveBookIds } from './reads'

// The books a screen follows on the stream: those typed, kept in the URL
// (`?books=BOOK-IG,BOOK-HY`), or else every live book the identity may list.

/** The book ids a typed list names, once each, in order: separated by commas or spaces. */
export function bookList(text: string): string[] {
  return [...new Set(text.split(/[\s,]+/).filter((id) => id.length > 0))]
}

export interface FollowedBooks {
  /** The most books followed: the topic's bound on one subscription. */
  most: number
  /** The books typed, all of them. */
  typed: string[]
  /** Every live book listed; null when the identity may not list books, undefined until read or when the read failed. */
  listed: string[] | null | undefined
  isListing: boolean
  listingError: Error | null
  /** The books followed: the first `most` typed, or else those listed. */
  bookIds: string[]
  /** Follow the books `text` names, or every live book when it names none. */
  follow: (text: string) => void
}

/** The books followed, at most `most`: a topic's bound on one subscription. */
export function useFollowedBooks(most: number): FollowedBooks {
  const { call } = useSession()
  const [search, setSearch] = useSearchParams()
  const typed = useMemo(() => bookList(search.get('books') ?? ''), [search])
  const listed = useQuery({
    queryKey: ['live-books', most],
    queryFn: () => readLiveBookIds(call, most),
    enabled: typed.length === 0,
  })
  const bookIds = useMemo(
    () => (typed.length > 0 ? typed.slice(0, most) : (listed.data ?? [])),
    [typed, listed.data, most],
  )
  const follow = useCallback(
    (text: string) => setSearch(text ? { books: bookList(text).join(',') } : {}),
    [setSearch],
  )
  return {
    most,
    typed,
    listed: listed.data,
    isListing: listed.isPending && typed.length === 0,
    listingError: listed.error,
    bookIds,
    follow,
  }
}
