#include "Enums.h"
#include "Game.h"
#include <stddef.h>

int main() {
    printf("%d\n%d\n%d\n%d\n%d\n", GAMEMODE, MAPMODE, TELEPORTERMODE, GAMECOMPLETE, GAMECOMPLETE2);
    printf("%zu\n", sizeof(Game));
    printf("%zu\n%zu\n%zu\n%zu\n%zu\n",
        offsetof(Game, roomx), offsetof(Game, roomy),
        offsetof(Game, state), offsetof(Game, gamestate),
        offsetof(Game, frames));
}
