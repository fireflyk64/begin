I want a text based space combat simulator in the vein of begin2.exe console
  mode but entirely written in rust language. begin2_annotated.asm has the
  annotated assembly and the manual-pdf.pdf has the commands and how the game
  works. I like the simplicity of the engine with fixed sized arrays to hold
  the ships but I do want to make it a bit more flexible so we can support
  near-future combat and things like battlestar with fighters and kinetic rail
  guns, and normal rounds.  For the environment we should use anise with
  de440.bsp and moons and minor planets like ceres from JPL orbital data and
  VSOP87 / jpl approximate planet position when moons are not configured.
  rings and asteroids should be procedurally generated with a fixed seed when
  you are near to the phenomenon. We should support multi player using ~/dev/lobbylink with these connections for a true peer to peer experience if players are playing multiplayer
  Environment should be configurable (with a maximum number of objects roughly
  equal to the data of the solar system). Clients should effectively be dumb
  terminals that can just see the text output

  [Image #1]
  [Image #2]
  [Image #3]
  [Image #4]

  The planet simulation part should be designed in from the beginning but
  should not occupy much of the code since it should be derived from the nasa
  data and be very self contained. the startup should just have the associated
  date. Space stations should be allowed and they should be able to be
  attached to moons or planets by name during simulation startup and at low or
  geosynchronous orbits but otherwise vessels should be spawned in near each
  other and with names of nearby planetary bodies (again low or high orbit) or
  phenomenon like rings


  in high orbit it should play almost exactly like begin since these
  maneuverable craft are able to accelerate far faster than the gravitational
  effects for the most part.

  one change from begin is that the starships will be moving in 3d with angles
  relative to the coordinate plane

  Remember what made begin so successful was the simplicity and
  understandability of it all. To start with ships should spawn on the same
  plane. Players can maneuver out of plane  like 320^22 but pursue should
  continue to pursue them out of plane, etc

  the game should have a planar lock mode where everyone is stuck to the same
  plane in case going out of plane makes missiles or railgun shots too easy to
  miss.  Remember that in the begin game things like phasers worked in a cone
  and torpedos had a very large splash area effect. railgun shots should
  travel between 0.01 and 0.1% speed of light, so the effects may be nearly
  instant and unavoidable if they are in the associated cone.
