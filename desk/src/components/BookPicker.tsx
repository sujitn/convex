import { useEffect, useState } from 'react'
import { failureNotice } from '../lib/failure'
import type { FollowedBooks } from '../lib/followedBooks'

/** What a screen follows: the books typed, or else every live book listed. */
function following({ most, typed, listed, isListing }: FollowedBooks): string | null {
  if (typed.length > most) return `the first ${most} of the books typed`
  if (typed.length > 0) return null
  if (isListing) return 'listing the books…'
  if (listed === null) return 'this identity may not list books: type the books to follow'
  if (listed === undefined) return 'the books could not be listed: type the books to follow'
  return `every live book (${listed.length})`
}

/** The books followed: those typed, kept in the URL, or else every live book listed, and why none could be. */
export function BookPicker({ books }: { books: FollowedBooks }) {
  const { typed, follow } = books
  const [draft, setDraft] = useState(typed.join(', '))
  useEffect(() => setDraft(typed.join(', ')), [typed])
  const shown = following(books)
  return (
    <form
      className="flex items-center gap-2"
      onSubmit={(event) => {
        event.preventDefault()
        follow(draft)
      }}
    >
      <input
        value={draft}
        placeholder="Books, e.g. BOOK-IG, BOOK-HY"
        onChange={(event) => setDraft(event.target.value)}
        className="w-64 rounded border border-slate-700 bg-slate-950 px-2 py-0.5 font-mono text-xs text-slate-100"
      />
      <button type="submit" className="rounded bg-slate-800 px-2 py-0.5 text-slate-200">
        Follow
      </button>
      {shown && <span>{shown}</span>}
      {books.listingError && <span className="text-red-400">{failureNotice(books.listingError)}</span>}
    </form>
  )
}
