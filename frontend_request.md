# Rematch Implementation Requirements

## Overview
This document outlines the implementation details for the rematch functionality in the Mines game. The rematch system allows players to restart a game with the same parameters after a game has finished.

## Message Types

### 1. Rematch Request
```typescript
interface RematchRequest {
    type: 'RematchRequest';
    game_id: string;
    requester: string;  // Player ID of the player requesting rematch
}
```

### 2. Rematch Response
```typescript
interface RematchResponse {
    type: 'RematchResponse';
    game_id: string;
    player_id: string;
    want_rematch: boolean;  // true for accept, false for decline
}
```

## Game States

### 1. REMATCH State
```typescript
interface RematchState {
    type: 'REMATCH';
    game_id: string;
    players: Player[];
    board: Board;
    single_bet_size: number;
    accepted: number[];  // Array tracking which players have accepted (1 for accepted, 0 for not yet)
}
```

## Implementation Requirements

### 1. UI Components Needed
- Rematch request button (visible only in FINISHED state)
- Rematch acceptance dialog/modal
- Rematch status indicator showing who has accepted
- Loading state during rematch transitions

### 2. State Management
- Track rematch acceptance status for all players
- Handle transition from FINISHED to REMATCH state
- Handle transition from REMATCH to either RUNNING or ABORTED state

### 3. User Flow
1. After game finishes (FINISHED state):
   - Show rematch request button to all players
   - Any player can initiate rematch

2. When rematch is requested:
   - Show rematch request notification to all players
   - Display acceptance dialog with accept/decline options
   - Show status of other players' responses

3. During REMATCH state:
   - Display which players have accepted
   - Show waiting message until all players respond
   - Disable game actions until rematch is resolved

4. Rematch Resolution:
   - If all accept: Transition to RUNNING state with new board
   - If any decline: Transition to ABORTED state

### 4. Error Handling
- Handle cases where player IDs are not found
- Handle network disconnections during rematch
- Provide appropriate error messages to users

### 5. Visual Feedback
- Clear indication of rematch request status
- Visual feedback for accepted/declined responses
- Loading indicators during state transitions
- Clear messaging for game abort due to declined rematch

## Technical Requirements

### 1. WebSocket Message Handling
```typescript
// Handle incoming rematch request
socket.on('RematchRequest', (data: RematchRequest) => {
    // Show rematch request UI
    // Enable accept/decline buttons
});

// Handle incoming rematch response
socket.on('RematchResponse', (data: RematchResponse) => {
    // Update rematch status UI
    // Check if all players have accepted
});

// Handle game state updates
socket.on('GameUpdate', (state: GameState) => {
    if (state.type === 'REMATCH') {
        // Update UI for rematch state
    }
});
```

### 2. State Transitions
```typescript
// Example state transition handling
function handleStateTransition(newState: GameState) {
    switch (newState.type) {
        case 'FINISHED':
            // Enable rematch request button
            break;
        case 'REMATCH':
            // Show rematch acceptance UI
            break;
        case 'RUNNING':
            // Start new game with same parameters
            break;
        case 'ABORTED':
            // Show game aborted message
            break;
    }
}
```

## Testing Requirements

1. Test Cases:
   - Single player requesting rematch
   - Multiple players accepting rematch
   - Player declining rematch
   - Network disconnection during rematch
   - All players accepting rematch
   - Mixed responses (some accept, some decline)

2. Edge Cases:
   - Player disconnects during rematch
   - Multiple rematch requests
   - Rematch request timing out
   - Invalid player responses

## Notes
- All game parameters (grid size, bomb count, bet size) remain the same in rematch
- Only the board is reinitialized with new bomb positions
- Player order and turn sequence remain the same
- Game can only be rematched from FINISHED state 