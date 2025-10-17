physics library
Simple one-dimensional physics simulations, such as springs, friction, and gravity, for use in user interface animations.

To use, import package:flutter/physics.dart.

Classes
BoundedFrictionSimulation
A FrictionSimulation that clamps the modeled particle to a specific range of values.
ClampedSimulation
A simulation that applies limits to another simulation.
FrictionSimulation
A simulation that applies a drag to slow a particle down.
GravitySimulation
A simulation that applies a constant accelerating force.
ScrollSpringSimulation
A SpringSimulation where the value of x is guaranteed to have exactly the end value when the simulation isDone.
Simulation
The base class for all simulations.
SpringDescription
Structure that describes a spring's constants.
SpringSimulation
A spring simulation.
Tolerance
Structure that specifies maximum allowable magnitudes for distances, durations, and velocity differences to be considered equal.
Enums
SpringType
The kind of spring solution that the SpringSimulation is using to simulate the spring.
Functions
nearEqual(double? a, double? b, double epsilon) → bool
Whether two doubles are within a given distance of each other.
nearZero(double a, double epsilon) → bool
Whether a double is within a given distance of zero.