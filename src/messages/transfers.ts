/**
 * The words for a file a peer wants to send.
 *
 * One question, two answers. The words are "accept" and "reject" rather than "yes" and "no" because the sentence above
 * them is what is being answered, and a bare "No" beside a peer's name reads as a judgement about the peer.
 */
export const transfers = {
  prompt: {
    title: 'A device wants to send a file',
    detail: 'Nothing is stored until you answer.',
    accept: 'Accept',
    reject: 'Reject',
    always: 'Always accept from this device'
  }
} as const
