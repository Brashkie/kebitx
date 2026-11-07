# function not_here
say hello
execute as @a at @s run function kx/other
summon kx:mob ~ ~ ~
give @s kx:gem 1
tellraw @a {"rawtext":[{"text":"function not_here"}]}
